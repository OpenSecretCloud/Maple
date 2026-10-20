use crate::provider_registry::{MapleInstanceId, ProviderId};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub provider_name: String,
}

impl std::fmt::Debug for ProxyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("provider_name", &self.provider_name)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ProxyRouter {
    default_proxy: ProxyConfig,
    tinfoil_proxy: ProxyConfig,
    maple_proxies: BTreeMap<MapleInstanceId, ProxyConfig>,
    maple_readiness: Option<crate::provider_client::MapleReadiness>,
}

impl ProxyRouter {
    pub fn new(openai_base: String, openai_key: Option<String>, tinfoil_base: String) -> Self {
        assert!(
            !tinfoil_base.trim().is_empty(),
            "Tinfoil API base must be configured"
        );

        let default_proxy = ProxyConfig {
            base_url: openai_base.clone(),
            api_key: if openai_base.contains("api.openai.com") {
                openai_key
            } else {
                None
            },
            provider_name: if openai_base.contains("api.openai.com") {
                "openai".to_string()
            } else {
                "continuum".to_string()
            },
        };

        let tinfoil_proxy = ProxyConfig {
            base_url: tinfoil_base,
            api_key: None,
            provider_name: "tinfoil".to_string(),
        };

        Self {
            default_proxy,
            tinfoil_proxy,
            maple_proxies: BTreeMap::new(),
            maple_readiness: None,
        }
    }

    pub(crate) fn with_maple_api_key(mut self, api_key: Option<String>) -> Self {
        self.maple_proxies = api_key
            .filter(|key| !key.trim().is_empty())
            .map(|key| {
                MapleInstanceId::ALL
                    .into_iter()
                    .map(|instance| {
                        let endpoint = crate::maple_inference::endpoint(instance);
                        (
                            instance,
                            ProxyConfig {
                                base_url: endpoint.base_url.to_owned(),
                                api_key: Some(key.clone()),
                                provider_name: ProviderId::Maple(instance).as_str().to_owned(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self
    }

    pub(crate) fn with_maple_readiness(
        mut self,
        readiness: crate::provider_client::MapleReadiness,
    ) -> Self {
        self.maple_readiness = Some(readiness);
        self
    }

    /// Configuration alone is insufficient: the direct client must have a
    /// currently verified transport. Checking also starts body-free renewal.
    pub(crate) fn get_maple_proxy(&self, instance: MapleInstanceId) -> Option<ProxyConfig> {
        let proxy = self.maple_proxies.get(&instance)?;
        self.maple_readiness
            .as_ref()?
            .is_ready(instance)
            .then(|| proxy.clone())
    }

    #[cfg(test)]
    pub(crate) fn maple_is_available(&self) -> bool {
        self.maple_is_configured()
            && self
                .maple_readiness
                .as_ref()
                .is_some_and(|readiness| readiness.any_ready())
    }

    #[cfg(test)]
    pub(crate) fn maple_is_configured(&self) -> bool {
        !self.maple_proxies.is_empty()
    }

    pub fn get_default_proxy(&self) -> ProxyConfig {
        self.default_proxy.clone()
    }

    /// The Continuum proxy, when the default inference proxy is one. System One is served
    /// only through it.
    pub fn continuum_proxy(&self) -> Option<ProxyConfig> {
        (self.default_proxy.provider_name == "continuum").then(|| self.default_proxy.clone())
    }

    pub fn get_completion_proxy(&self) -> ProxyConfig {
        self.tinfoil_proxy.clone()
    }

    pub fn get_tinfoil_proxy(&self) -> ProxyConfig {
        self.tinfoil_proxy.clone()
    }

    #[cfg(test)]
    pub fn get_tinfoil_base_url(&self) -> String {
        self.tinfoil_proxy.base_url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dedicated_router() -> ProxyRouter {
        ProxyRouter::new(
            "http://continuum.example.com".to_owned(),
            None,
            "http://tinfoil.example.com".to_owned(),
        )
    }

    #[test]
    fn maple_proxies_require_own_key_and_each_guests_verified_readiness() {
        for key in [None, Some("".to_owned()), Some("   ".to_owned())] {
            let router = dedicated_router()
                .with_maple_api_key(key)
                .with_maple_readiness(crate::provider_client::MapleReadiness::for_test(true));
            assert!(!router.maple_is_configured());
            assert!(!router.maple_is_available());
            for instance in MapleInstanceId::ALL {
                assert!(router.get_maple_proxy(instance).is_none());
            }
        }
        let configured = dedicated_router().with_maple_api_key(Some("dedicated-key".to_owned()));
        assert!(configured.maple_is_configured());
        assert!(!configured.maple_is_available());
        for instance in MapleInstanceId::ALL {
            assert!(configured.get_maple_proxy(instance).is_none());
        }
        for ready in MapleInstanceId::ALL {
            let router = configured.clone().with_maple_readiness(
                crate::provider_client::MapleReadiness::for_test_instances(&[ready]),
            );
            assert!(router.maple_is_available());
            for instance in MapleInstanceId::ALL {
                let proxy = router.get_maple_proxy(instance);
                assert_eq!(proxy.is_some(), instance == ready);
                if let Some(proxy) = proxy {
                    assert_eq!(proxy.provider_name, ProviderId::Maple(instance).as_str());
                    assert_eq!(
                        proxy.base_url,
                        crate::maple_inference::endpoint(instance).base_url
                    );
                    assert_eq!(proxy.api_key.as_deref(), Some("dedicated-key"));
                    assert!(!format!("{proxy:?}").contains("dedicated-key"));
                }
            }
        }
        let router = configured
            .with_maple_readiness(crate::provider_client::MapleReadiness::for_test(false));
        assert!(router.maple_is_configured());
        assert!(!router.maple_is_available());
    }

    #[test]
    fn test_proxy_router_uses_openai_api_key_only_for_openai() {
        let router = ProxyRouter::new(
            "https://api.openai.com".to_string(),
            Some("test-key".to_string()),
            "http://tinfoil.example.com".to_string(),
        );

        assert_eq!(router.get_default_proxy().provider_name, "openai");
        assert_eq!(
            router.get_default_proxy().api_key,
            Some("test-key".to_string())
        );

        let continuum_router = ProxyRouter::new(
            "http://continuum.example.com".to_string(),
            Some("ignored".to_string()),
            "http://tinfoil.example.com".to_string(),
        );

        assert_eq!(
            continuum_router.get_default_proxy().provider_name,
            "continuum"
        );
        assert_eq!(continuum_router.get_default_proxy().api_key, None);
    }

    #[test]
    fn test_completion_proxy_prefers_tinfoil_when_configured() {
        let router = ProxyRouter::new(
            "http://continuum.example.com".to_string(),
            None,
            "http://tinfoil.example.com".to_string(),
        );

        assert_eq!(router.get_completion_proxy().provider_name, "tinfoil");
        assert_eq!(
            router.get_tinfoil_base_url(),
            "http://tinfoil.example.com".to_string()
        );
    }

    #[test]
    #[should_panic(expected = "Tinfoil API base must be configured")]
    fn test_proxy_router_requires_tinfoil() {
        let _router = ProxyRouter::new(
            "http://continuum.example.com".to_string(),
            None,
            String::new(),
        );
    }

    #[test]
    fn test_completion_proxy_uses_tinfoil_even_with_continuum_default() {
        let router = ProxyRouter::new(
            "http://continuum.example.com".to_string(),
            None,
            "http://tinfoil.example.com".to_string(),
        );

        assert_eq!(router.get_completion_proxy().provider_name, "tinfoil");
        assert_eq!(router.get_tinfoil_proxy().provider_name, "tinfoil");
    }
}
