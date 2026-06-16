const PASSWORD_RESET_HARDENING_NOTICE_ENABLED_AT_MS = Date.parse("2026-06-18T06:00:00.000Z");

export function isPasswordResetHardeningNoticeEnabled(nowMs = Date.now()) {
  return nowMs >= PASSWORD_RESET_HARDENING_NOTICE_ENABLED_AT_MS;
}
