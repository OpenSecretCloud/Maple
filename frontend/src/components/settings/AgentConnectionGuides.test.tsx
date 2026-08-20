import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentConnectionGuides } from "./AgentConnectionGuides";
import type { MapleAcpStatus } from "@/services/mapleAcpService";

const status: MapleAcpStatus = {
  running: true,
  enabled: true,
  connectedClients: 1,
  activeSessions: 1,
  activeRuns: 0,
  endpoint: "/tmp/maple.sock",
  endpointKind: "unix_socket",
  protocolVersion: 1,
  error: null,
  buzzCredentialsAvailable: false,
  harness: {
    command: "/Applications/Maple.app/Contents/MacOS/maple",
    args: ["acp"]
  }
};

describe("AgentConnectionGuides", () => {
  test("leads with Paseo and keeps Buzz as a client-specific tab", () => {
    const markup = renderToStaticMarkup(<AgentConnectionGuides status={status} />);

    expect(markup).toContain(">Paseo</button>");
    expect(markup).toContain(">Buzz</button>");
    expect(markup.indexOf(">Paseo</button>")).toBeLessThan(markup.indexOf(">Buzz</button>"));
    expect(markup).toContain("Paseo provider configuration");
    expect(markup).toContain("~/.paseo/config.json");
    expect(markup).toContain("maple-acp");
    expect(markup).toContain("supportsMcpServers");
  });
});
