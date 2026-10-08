#if os(iOS)
    import MapleAppCore

    enum AttestationClockText {
        static func message(_ issue: AttestationClockIssue) -> String {
            switch issue {
            case .deviceTime:
                MapleStrings.text(
                    "This device’s date or time appears incorrect. Check Date & Time and the time zone in Settings, then try again."
                )
            case .certificateTiming:
                MapleStrings.text(
                    "The secure connection’s certificate is not valid at the device’s current date and time. Try again in a moment. If this continues, check Date & Time and the time zone in Settings."
                )
            }
        }
    }
#endif
