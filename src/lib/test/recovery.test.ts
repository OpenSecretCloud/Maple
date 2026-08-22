import { describe, expect, test } from "bun:test";
import {
  classifyRecovery,
  ERROR_CODE_HEADER,
  ERROR_CONTRACT_HEADER,
  type RecoveryAction
} from "../recovery";

interface RecoveryCase {
  name: string;
  status: number;
  contract?: string;
  code?: string;
  expected?: RecoveryAction;
}

const cases: RecoveryCase[] = [
  { name: "legacy 400", status: 400, expected: "renew_session" },
  { name: "legacy 401", status: 401, expected: "refresh_access_token" },
  { name: "legacy other status", status: 422 },
  {
    name: "v1 session not found",
    status: 400,
    contract: "1",
    code: "session_not_found",
    expected: "renew_session"
  },
  {
    name: "v1 access token expired",
    status: 401,
    contract: "1",
    code: "access_token_expired",
    expected: "refresh_access_token"
  },
  { name: "v1 missing code", status: 400, contract: "1" },
  { name: "v1 unknown code", status: 400, contract: "1", code: "other" },
  {
    name: "v1 malformed code",
    status: 400,
    contract: "1",
    code: "session_not_found;foo"
  },
  {
    name: "v1 session code with wrong status",
    status: 401,
    contract: "1",
    code: "session_not_found"
  },
  {
    name: "v1 access code with wrong status",
    status: 400,
    contract: "1",
    code: "access_token_expired"
  },
  { name: "empty contract marker", status: 400, contract: "" },
  {
    name: "malformed contract marker",
    status: 400,
    contract: "1;foo",
    code: "session_not_found"
  },
  {
    name: "future contract marker",
    status: 400,
    contract: "2",
    code: "session_not_found"
  },
  {
    name: "legacy marker absence ignores an unversioned code",
    status: 400,
    code: "unknown",
    expected: "renew_session"
  }
];

describe("recovery contract classifier", () => {
  for (const recoveryCase of cases) {
    test(recoveryCase.name, () => {
      const headers = new Headers();
      if (recoveryCase.contract !== undefined) {
        headers.set(ERROR_CONTRACT_HEADER, recoveryCase.contract);
      }
      if (recoveryCase.code !== undefined) {
        headers.set(ERROR_CODE_HEADER, recoveryCase.code);
      }

      expect(classifyRecovery(recoveryCase.status, headers)).toBe(recoveryCase.expected);
    });
  }

  test("duplicate contract markers fail closed", () => {
    const headers = new Headers();
    headers.append(ERROR_CONTRACT_HEADER, "1");
    headers.append(ERROR_CONTRACT_HEADER, "1");
    headers.set(ERROR_CODE_HEADER, "session_not_found");

    expect(classifyRecovery(400, headers)).toBeUndefined();
  });

  test("duplicate error codes fail closed", () => {
    const headers = new Headers();
    headers.set(ERROR_CONTRACT_HEADER, "1");
    headers.append(ERROR_CODE_HEADER, "session_not_found");
    headers.append(ERROR_CODE_HEADER, "session_not_found");

    expect(classifyRecovery(400, headers)).toBeUndefined();
  });
});
