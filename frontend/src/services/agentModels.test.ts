import { describe, expect, test } from "bun:test";
import { POWERFUL_MODEL_ALIAS, QUICK_MODEL_ALIAS } from "@/utils/utils";
import {
  DEFAULT_AGENT_MODEL,
  PRIMARY_AGENT_MODEL_IDS,
  fallbackAgentModel,
  reconcileAgentModel
} from "./agentModels";

const models = [{ id: DEFAULT_AGENT_MODEL }, { id: "kimi-k2-6" }];

describe("Agent Mode model defaults", () => {
  test("promotes GLM first and leaves Kimi out of the primary choices", () => {
    expect(PRIMARY_AGENT_MODEL_IDS).toEqual([DEFAULT_AGENT_MODEL, QUICK_MODEL_ALIAS]);
    expect(PRIMARY_AGENT_MODEL_IDS).not.toContain("kimi-k2-6");
  });

  test("falls back to GLM when it is available, otherwise Quick", () => {
    expect(fallbackAgentModel(models)).toBe(DEFAULT_AGENT_MODEL);
    expect(fallbackAgentModel([{ id: "kimi-k2-6" }])).toBe(QUICK_MODEL_ALIAS);
  });

  test("keeps selectable concrete models and existing aliases", () => {
    expect(reconcileAgentModel("kimi-k2-6", models)).toBe("kimi-k2-6");
    expect(reconcileAgentModel(QUICK_MODEL_ALIAS, models)).toBe(QUICK_MODEL_ALIAS);
    expect(reconcileAgentModel(POWERFUL_MODEL_ALIAS, models)).toBe(POWERFUL_MODEL_ALIAS);
  });

  test("replaces a missing concrete model with the best available default", () => {
    expect(reconcileAgentModel("retired-model", models)).toBe(DEFAULT_AGENT_MODEL);
    expect(reconcileAgentModel(DEFAULT_AGENT_MODEL, [{ id: "kimi-k2-6" }])).toBe(QUICK_MODEL_ALIAS);
  });
});
