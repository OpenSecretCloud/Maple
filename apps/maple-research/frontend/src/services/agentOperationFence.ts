export class AgentOperationsBlockedError extends Error {
  constructor() {
    super("Agent Mode is stopping for this account");
    this.name = "AgentOperationsBlockedError";
  }
}

export interface AgentOperationBlock {
  release(): void;
  retainUntilNextSession(): void;
}

interface UserOperationState {
  generation: number;
  blockers: Map<symbol, { releaseOnActivation: boolean }>;
  inFlight: Set<Promise<unknown>>;
}

export class AgentOperationFence {
  private readonly users = new Map<string, UserOperationState>();

  async run<T>(userId: string, operation: () => Promise<T>): Promise<T> {
    const state = this.stateFor(userId);
    if (state.blockers.size > 0) throw new AgentOperationsBlockedError();

    const generation = state.generation;
    const tracked = Promise.resolve().then(async () => {
      if (state.blockers.size > 0 || state.generation !== generation) {
        throw new AgentOperationsBlockedError();
      }
      return await operation();
    });
    state.inFlight.add(tracked);

    try {
      return await tracked;
    } finally {
      state.inFlight.delete(tracked);
    }
  }

  async blockAndDrain(userId: string): Promise<AgentOperationBlock> {
    const state = this.stateFor(userId);
    const token = Symbol("agent-operation-block");
    state.blockers.set(token, { releaseOnActivation: false });
    state.generation += 1;

    while (state.inFlight.size > 0) {
      await Promise.allSettled([...state.inFlight]);
    }

    let released = false;
    return {
      release: () => {
        if (released) return;
        released = true;
        state.blockers.delete(token);
      },
      retainUntilNextSession: () => {
        if (released) return;
        const blocker = state.blockers.get(token);
        if (blocker) blocker.releaseOnActivation = true;
      }
    };
  }

  activateUserSession(userId: string): void {
    const state = this.stateFor(userId);
    if (state.blockers.size === 0) return;
    for (const [token, blocker] of state.blockers) {
      if (blocker.releaseOnActivation) state.blockers.delete(token);
    }
    state.generation += 1;
  }

  private stateFor(userId: string): UserOperationState {
    if (!userId.trim()) throw new Error("Agent operations require an authenticated user");

    let state = this.users.get(userId);
    if (!state) {
      state = {
        generation: 0,
        blockers: new Map(),
        inFlight: new Set()
      };
      this.users.set(userId, state);
    }
    return state;
  }
}

export const agentOperationFence = new AgentOperationFence();
