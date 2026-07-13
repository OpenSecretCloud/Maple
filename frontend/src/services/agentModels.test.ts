import { describe, expect, test } from "bun:test";
import { POWERFUL_MODEL_ALIAS, QUICK_MODEL_ALIAS } from "@/utils/utils";
import {
  DEFAULT_AGENT_MODEL,
  PRIMARY_AGENT_MODEL_IDS,
  fallbackAgentModel,
  reconcileAgentModel,
  resolveAgentModelVisionCapability
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

describe("resolveAgentModelVisionCapability", () => {
  const catalog = [
    { id: "glm", capabilities: { vision: false } },
    { id: "gemma", capabilities: { vision: true } }
  ];

  test("uses the concrete model capability", () => {
    expect(resolveAgentModelVisionCapability("gemma", catalog, [])).toBe(true);
    expect(resolveAgentModelVisionCapability("glm", catalog, [])).toBe(false);
  });

  test("resolves aliases through their current catalog target", () => {
    const aliases = [
      {
        id: QUICK_MODEL_ALIAS,
        target_model: "gemma",
        capabilities: { vision: false }
      }
    ];

    expect(resolveAgentModelVisionCapability(QUICK_MODEL_ALIAS, catalog, aliases)).toBe(true);
  });

  test("uses alias metadata when its target is unavailable", () => {
    const aliases = [
      {
        id: POWERFUL_MODEL_ALIAS,
        target_model: "missing",
        capabilities: { vision: true }
      }
    ];

    expect(resolveAgentModelVisionCapability(POWERFUL_MODEL_ALIAS, catalog, aliases)).toBe(true);
  });

  test("fails closed when the model capability is unknown", () => {
    expect(resolveAgentModelVisionCapability("unknown", catalog, [])).toBe(false);
    expect(resolveAgentModelVisionCapability(QUICK_MODEL_ALIAS, catalog, [])).toBe(false);
  });
});
