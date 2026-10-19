//! Caller-owned identities for Maple's dedicated attested inference endpoint.
//!
//! Changing a node's measured release requires reviewing these pins before the
//! node update. DNS, the API credential, or a server-supplied release reference
//! never authorizes a new measurement. This is ordinary attested HTTPS using
//! Tinfoil's direct V3 path; it does not use shared-router discovery.

use tinfoil::{ApprovedRelease, DirectPolicy, Measurement, PinnedRelease, PredicateType};

pub(crate) const HOST: &str = "i01.tfinf.maple.ai";
pub(crate) const BASE_URL: &str = "https://i01.tfinf.maple.ai";

struct ReleasePin {
    repo: &'static str,
    tag: &'static str,
    digest: &'static str,
    registers: &'static [&'static str],
}

// Independently qualified own-node release; these identify the published code
// artifact and full multiplatform launch measurement, not a cached live quote.
// Keep the current release accepted while an explicitly reviewed staged release
// is deployed. Remove retired entries after the rollout completes.
const CURRENT_RELEASE: ReleasePin = ReleasePin {
    repo: "MaplePrivacyLabs/inference-deepseek",
    tag: "v0.1.0",
    digest: "2d69e8544e5b2706d0e6004bc0ac925dc9abd79cb284f9400c26c9d145e25b00",
    registers: &[
        "d4d5fd1166413a3229a83899b99c06fb8cb904badb2ed5b9b8e6cb7414ad110628b6780eddaf10c42cbc996909145541",
        "7e57f9877b4533e7dcb8dd7419dfaf422f3c7cef6fd337a4eaf27ae90a751ff04945ef0d4fb6c3753285cb1f09ce048e",
        "24e53d3e8a77e9aa9182b61bbddc5138050c5dc00628231c51cfe6798431f64d59af20494708190c7aca85b166675550",
    ],
};
const STAGED_RELEASES: &[ReleasePin] = &[];

pub(crate) fn direct_policy() -> Result<DirectPolicy, tinfoil::Error> {
    let approved = std::iter::once(&CURRENT_RELEASE)
        .chain(STAGED_RELEASES.iter())
        .map(|pin| ApprovedRelease {
            release: PinnedRelease {
                repo: pin.repo.to_owned(),
                tag: pin.tag.to_owned(),
                digest: pin.digest.to_owned(),
            },
            measurement: Measurement {
                type_: PredicateType::SnpTdxMultiPlatformV1,
                registers: pin
                    .registers
                    .iter()
                    .map(|register| (*register).to_owned())
                    .collect(),
            },
        })
        .collect();
    DirectPolicy::new(approved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedicated_policy_is_complete_and_explicit() {
        let policy =
            direct_policy().expect("compiled current/staged pins must form a valid policy");
        let serialized = format!("{policy:?}");
        assert!(serialized.contains(CURRENT_RELEASE.repo));
        assert!(serialized.contains(CURRENT_RELEASE.tag));
        assert!(serialized.contains(CURRENT_RELEASE.digest));
        for register in CURRENT_RELEASE.registers {
            assert!(serialized.contains(register));
        }
        let url = reqwest::Url::parse(BASE_URL).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some(HOST));
        assert!(url.port().is_none());
        assert_eq!(url.path(), "/");
    }
}
