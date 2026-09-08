import { useQuery } from "@tanstack/react-query";
import { useOpenSecret } from "@opensecret/react";

export type ApiKeySummary = {
  name: string;
  created_at: string;
};

export function useApiKeys() {
  const { auth, listApiKeys } = useOpenSecret();

  return useQuery<ApiKeySummary[]>({
    queryKey: ["apiKeys"],
    queryFn: async () => {
      const response = await listApiKeys();
      return response.keys.sort(
        (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
      );
    },
    enabled: !!auth.user && !auth.loading
  });
}
