// Embedded OpenSecret PCR0 roots shared with the TypeScript and Rust SDKs.
// Changes to approvals belong to the existing signed-PCR review process.
extension PCRTrustPolicy {
    static let productionRoots: [String] = [
        // Approved 2026-10-11 (services/opensecret signed history)
        "21d27bd19cdd02632485a6e8874753e42cf7f098f1335bb62a9c23e7f64da9100c02dba582da17ac68052fa6be16b7a9",
        // Approved 2026-10-09 (services/opensecret signed history; rollback)
        "643b3c293a4eac11ec14ab405d2405e46cb714263a12da6c62f485a0fa395cb3c3d5ef07b18d39a1f9715f25e7943b66",
    ]

    static let developmentRoots: [String] = [
        // Approved 2026-10-11 (services/opensecret signed history)
        "a95956c3fb9457e509be6d614ffefcddf98fe594f77bb6a56891fc1f5bdec41da2add13db22e0908363c8d0929c04def",
        // Approved 2026-10-09 (services/opensecret signed history; rollback)
        "e573a3ddc8a27f387e24cfb02a8fa594fe782674de6ee1fa9385126b9f8ec38f4a236e91c371bdd2b9444623c1c1cc34",
    ]
}
