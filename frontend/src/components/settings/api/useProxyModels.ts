import { useQuery } from "@tanstack/react-query";
import { useOpenSecret } from "@opensecret/react";
import { fetchProxyChatModels } from "@/services/proxyModels";
import type { OpenSecretModel, OpenSecretModelCatalog } from "@/state/LocalStateContextDef";

export function useProxyModels() {
  const { auth, fetchModelCatalog, fetchModels } = useOpenSecret();

  return useQuery<OpenSecretModel[]>({
    queryKey: ["proxy-chat-model-catalog"],
    queryFn: () =>
      fetchProxyChatModels({
        fetchModelCatalog: async () =>
          (await fetchModelCatalog()) as unknown as OpenSecretModelCatalog,
        fetchModels: async () => (await fetchModels()) as OpenSecretModel[]
      }),
    enabled: !!auth.user && !auth.loading,
    staleTime: 5 * 60 * 1000
  });
}
