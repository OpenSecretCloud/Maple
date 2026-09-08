import { describe, expect, test } from "bun:test";
import { APIConnectionError } from "openai";
import { isImageDescriptionUnavailableError } from "./chatResponseErrors";

function codedError(
  status = 503,
  contract = "1",
  code = "image_description_unavailable"
): Error & { status: number; headers: Headers } {
  return Object.assign(new Error("Request failed"), {
    status,
    headers: new Headers({
      "x-opensecret-error-contract": contract,
      "x-opensecret-error-code": code
    })
  });
}

describe("Responses error classification", () => {
  test("recognizes the coded descriptor failure through the OpenAI connection wrapper", () => {
    const error = new APIConnectionError({ cause: codedError() });

    expect(isImageDescriptionUnavailableError(error)).toBe(true);
  });

  test("recognizes a top-level OpenAI-style HTTP error", () => {
    expect(isImageDescriptionUnavailableError(codedError())).toBe(true);
  });

  test("fails closed for unrelated or malformed errors", () => {
    expect(isImageDescriptionUnavailableError(codedError(500))).toBe(false);
    expect(isImageDescriptionUnavailableError(codedError(503, "2"))).toBe(false);
    expect(isImageDescriptionUnavailableError(codedError(503, "1", "other_error"))).toBe(false);
    expect(
      isImageDescriptionUnavailableError(
        Object.assign(new Error("missing contract"), {
          status: 503,
          headers: new Headers({
            "x-opensecret-error-code": "image_description_unavailable"
          })
        })
      )
    ).toBe(false);
    expect(isImageDescriptionUnavailableError(new Error("ordinary failure"))).toBe(false);
  });
});
