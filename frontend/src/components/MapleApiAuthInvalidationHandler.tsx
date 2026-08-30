import { useEffect, useRef } from "react";
import { useOpenSecret } from "@opensecret/react";
import { mapleApiAuthService, type MapleApiAuthInvalidated } from "@/services/mapleApiAuthService";

interface MapleApiAuthInvalidationSource {
  subscribeInvalidation(handler: (event: MapleApiAuthInvalidated) => void): () => void;
}

export function MapleApiAuthInvalidationHandler({
  source = mapleApiAuthService
}: {
  source?: MapleApiAuthInvalidationSource;
}) {
  const os = useOpenSecret();
  const userIdRef = useRef<string | null>(null);
  const signOutRef = useRef(os.signOut);
  const invalidatingUserIdRef = useRef<string | null>(null);
  userIdRef.current = os.auth.user?.user.id.toLowerCase() ?? null;
  signOutRef.current = os.signOut;

  useEffect(
    () =>
      source.subscribeInvalidation(({ userId }) => {
        const normalizedUserId = userId.trim().toLowerCase();
        if (
          !normalizedUserId ||
          userIdRef.current !== normalizedUserId ||
          invalidatingUserIdRef.current === normalizedUserId
        ) {
          return;
        }

        invalidatingUserIdRef.current = normalizedUserId;
        void signOutRef
          .current()
          .catch(() => undefined)
          .finally(() => {
            if (invalidatingUserIdRef.current === normalizedUserId) {
              invalidatingUserIdRef.current = null;
            }
          });
      }),
    [source]
  );

  return null;
}
