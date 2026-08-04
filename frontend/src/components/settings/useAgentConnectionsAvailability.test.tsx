import { afterEach, describe, expect, mock, spyOn, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { FEATURE_FLAGS, FlagsClient } from "@/services/flags";
import {
  AgentConnectionsAvailabilityProvider,
  useAgentConnectionsAvailability,
  type AgentConnectionsAvailability,
  type AgentConnectionsAvailabilityDependencies
} from "./useAgentConnectionsAvailability";

const USER_A = "00000000-0000-0000-0000-000000000001";
const USER_B = "00000000-0000-0000-0000-000000000002";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function AvailabilityProbe({
  dependencies,
  userId
}: {
  dependencies: AgentConnectionsAvailabilityDependencies;
  userId: string | null;
}) {
  return (
    <AgentConnectionsAvailabilityProvider dependencies={dependencies} userId={userId}>
      <AvailabilityValue />
    </AgentConnectionsAvailabilityProvider>
  );
}

function AvailabilityValue() {
  const availability = useAgentConnectionsAvailability();
  return <span>{availability}</span>;
}

function renderedAvailability(renderer: ReactTestRenderer): AgentConnectionsAvailability {
  const [availability] = renderer.root.findByType("span").children;
  if (
    availability !== "checking" &&
    availability !== "available" &&
    availability !== "unavailable"
  ) {
    throw new Error("Availability probe did not render a valid state");
  }
  return availability;
}

function dependencies({
  cached,
  enabled,
  platformSupported = true
}: {
  cached?: boolean;
  enabled?: (userId: string, key: string) => Promise<boolean>;
  platformSupported?: boolean;
} = {}) {
  const peekIsEnabled = mock(() => cached);
  const isEnabled = mock(enabled ?? (async () => cached === true));
  return {
    dependencies: {
      flagClient: { isEnabled, peekIsEnabled },
      isPlatformSupported: () => platformSupported
    },
    isEnabled,
    peekIsEnabled
  };
}

describe("useAgentConnectionsAvailability", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("does not consult flags without a supported platform and authenticated user", () => {
    const unsupported = dependencies({ platformSupported: false });

    act(() => {
      renderer = create(
        <AvailabilityProbe dependencies={unsupported.dependencies} userId={USER_A} />
      );
    });

    expect(renderedAvailability(renderer!)).toBe("unavailable");
    expect(unsupported.peekIsEnabled).not.toHaveBeenCalled();
    expect(unsupported.isEnabled).not.toHaveBeenCalled();

    const supported = dependencies();
    act(() => {
      renderer?.update(<AvailabilityProbe dependencies={supported.dependencies} userId={null} />);
    });

    expect(renderedAvailability(renderer!)).toBe("unavailable");
    expect(supported.peekIsEnabled).not.toHaveBeenCalled();
    expect(supported.isEnabled).not.toHaveBeenCalled();
  });

  test("waits for a cold remote lookup before admitting the surface", async () => {
    const lookup = deferred<boolean>();
    const remote = dependencies({ enabled: () => lookup.promise });

    act(() => {
      renderer = create(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_A} />);
    });

    expect(renderedAvailability(renderer!)).toBe("checking");
    expect(remote.peekIsEnabled).toHaveBeenCalledWith(USER_A, FEATURE_FLAGS.AGENT_CONNECTIONS);
    expect(remote.isEnabled).toHaveBeenCalledWith(USER_A, FEATURE_FLAGS.AGENT_CONNECTIONS);

    await act(async () => {
      lookup.resolve(true);
      await lookup.promise;
    });

    expect(renderedAvailability(renderer!)).toBe("available");
  });

  test("keeps a remotely disabled surface unavailable", async () => {
    const lookup = deferred<boolean>();
    const remote = dependencies({ enabled: () => lookup.promise });

    act(() => {
      renderer = create(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_A} />);
    });

    await act(async () => {
      lookup.resolve(false);
      await lookup.promise;
    });

    expect(renderedAvailability(renderer!)).toBe("unavailable");
  });

  test("uses a cached value on the first render", () => {
    const cached = dependencies({ cached: true });

    act(() => {
      renderer = create(<AvailabilityProbe dependencies={cached.dependencies} userId={USER_A} />);
    });

    expect(renderedAvailability(renderer!)).toBe("available");
  });

  test("honors the real local override without calling the remote service", () => {
    const env = import.meta.env as { VITE_FORCE_FEATURE_FLAGS?: string };
    const previousOverride = env.VITE_FORCE_FEATURE_FLAGS;
    const fetchFn = mock(async () => {
      throw new Error("Local override should not fetch remote flags");
    });
    const localClient = new FlagsClient({
      baseUrl: "https://flags.example.test",
      fetchFn
    });
    env.VITE_FORCE_FEATURE_FLAGS = [previousOverride, FEATURE_FLAGS.AGENT_CONNECTIONS]
      .filter(Boolean)
      .join(",");

    try {
      act(() => {
        renderer = create(
          <AvailabilityProbe
            dependencies={{
              flagClient: localClient,
              isPlatformSupported: () => true
            }}
            userId={USER_A}
          />
        );
      });

      expect(renderedAvailability(renderer!)).toBe("available");
      expect(fetchFn).not.toHaveBeenCalled();
    } finally {
      if (previousOverride === undefined) delete env.VITE_FORCE_FEATURE_FLAGS;
      else env.VITE_FORCE_FEATURE_FLAGS = previousOverride;
    }
  });

  test("fails closed when the remote lookup errors", async () => {
    const lookup = deferred<boolean>();
    const remote = dependencies({ enabled: () => lookup.promise });
    const warning = spyOn(console, "warn").mockImplementation(() => {});

    act(() => {
      renderer = create(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_A} />);
    });

    await act(async () => {
      lookup.reject(new Error("flags unavailable"));
      await lookup.promise.catch(() => undefined);
    });

    expect(renderedAvailability(renderer!)).toBe("unavailable");
    expect(warning).toHaveBeenCalledTimes(1);
    warning.mockRestore();
  });

  test("ignores a stale lookup after the authenticated user changes", async () => {
    const userA = deferred<boolean>();
    const userB = deferred<boolean>();
    const remote = dependencies({
      enabled: (userId) => (userId === USER_A ? userA.promise : userB.promise)
    });

    act(() => {
      renderer = create(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_A} />);
    });
    act(() => {
      renderer?.update(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_B} />);
    });

    await act(async () => {
      userA.resolve(true);
      await userA.promise;
    });
    expect(renderedAvailability(renderer!)).toBe("checking");

    await act(async () => {
      userB.resolve(false);
      await userB.promise;
    });
    expect(renderedAvailability(renderer!)).toBe("unavailable");
  });

  test("does not resurrect an old result after an A to B to A transition", async () => {
    const firstUserA = deferred<boolean>();
    const secondUserA = deferred<boolean>();
    const userB = deferred<boolean>();
    let userALookups = 0;
    const remote = dependencies({
      enabled: (userId) => {
        if (userId === USER_B) return userB.promise;
        userALookups += 1;
        return userALookups === 1 ? firstUserA.promise : secondUserA.promise;
      }
    });

    act(() => {
      renderer = create(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_A} />);
    });
    await act(async () => {
      firstUserA.resolve(true);
      await firstUserA.promise;
    });
    expect(renderedAvailability(renderer!)).toBe("available");

    act(() => {
      renderer?.update(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_B} />);
    });
    act(() => {
      renderer?.update(<AvailabilityProbe dependencies={remote.dependencies} userId={USER_A} />);
    });

    expect(renderedAvailability(renderer!)).toBe("checking");

    await act(async () => {
      secondUserA.resolve(false);
      await secondUserA.promise;
    });
    expect(renderedAvailability(renderer!)).toBe("unavailable");
    expect(userALookups).toBe(2);
  });
});
