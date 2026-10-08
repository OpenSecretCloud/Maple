import XCTest

/// UI ownership/form evidence; encrypted transport and server contracts have a
/// separate local integration gate. This fixture never contacts a real provider.
@MainActor final class AuthWorkflowUITests: XCTestCase {
    private var app: XCUIApplication!

    private func launchFixture(clockFailure: Bool = false, expiry: Bool = false) {
        continueAfterFailure = false
        app = XCUIApplication(); app.launchArguments = ["--maple-auth-workflow-fixture"]
        if expiry { app.launchArguments.append("--maple-auth-expiry-fixture") }
        if clockFailure { app.launchArguments.append("--maple-auth-clock-fixture") }
        app.launch()
        waitLabel("auth.fixture.phase", "signedOut")
    }
    private func waitLabel(_ identifier: String, _ value: String) {
        let element = app.staticTexts[identifier]
        XCTAssertTrue(element.waitForExistence(timeout: 15))
        let changed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", value), object: element)
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 15), .completed)
    }
    private func tap(_ identifier: String) {
        let button = app.buttons[identifier]
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        if !button.isHittable { app.swipeUp() }
        button.tap()
    }
    private func text(_ identifier: String, _ value: String, secure: Bool = false) {
        let field = secure ? app.secureTextFields[identifier] : app.textFields[identifier]
        XCTAssertTrue(field.waitForExistence(timeout: 10)); field.tap(); field.typeText(value)
        guard secure else { return }
        // Inspect native secure entry before submitting; system password assistance
        // can interrupt typing. Preserve the evidence and the bounded real retry
        // so form validation is never bypassed or silently discounted.
        let lengthPredicate = NSPredicate(format: "value MATCHES %@", "^.{\(value.utf16.count)}$")
        let initial = XCTNSPredicateExpectation(predicate: lengthPredicate, object: field)
        if XCTWaiter.wait(for: [initial], timeout: 2) != .completed {
            let current = (field.value as? String) ?? ""
            let length = current == field.placeholderValue ? 0 : min(current.utf16.count, 256)
            let evidence = XCTAttachment(
                string: "Native secure entry length \(length); expected \(value.utf16.count); bounded typing retry")
            evidence.name = "Secure field entry synchronization"; evidence.lifetime = .keepAlways; add(evidence)
            let hierarchy = XCTAttachment(string: app.debugDescription)
            hierarchy.name = "Synthetic secure-entry app hierarchy"; hierarchy.lifetime = .keepAlways; add(hierarchy)
            let suggestions = app.windows.containing(.button, identifier: "GenerateStrongPasswordButton")
            if suggestions.count > 0 || app.staticTexts["Use Strong Password?"].exists {
                XCTAssertEqual(suggestions.count, 1)
                let suggestion = suggestions.element(boundBy: 0)
                XCTAssertTrue(suggestion.staticTexts["Use Strong Password?"].exists)
                XCTAssertEqual(suggestion.buttons["GenerateStrongPasswordButton"].label, "Fill Strong Password")
                let close = suggestion.buttons.matching(
                    NSPredicate(format: "identifier == %@ AND label == %@", "xmark", "Close"))
                XCTAssertEqual(close.count, 1)
                XCTAssertTrue(close.element.isEnabled)
                XCTAssertTrue(close.element.isHittable)
                close.element.tap()
                let dismissed = XCTNSPredicateExpectation(
                    predicate: NSPredicate { _, _ in
                        suggestions.count == 0 && !self.app.staticTexts["Use Strong Password?"].exists
                    }, object: app)
                XCTAssertEqual(XCTWaiter.wait(for: [dismissed], timeout: 5), .completed)
            }
            let retryValue = (field.value as? String) ?? ""
            let retryLength = retryValue == field.placeholderValue ? 0 : min(retryValue.utf16.count, 256)
            if retryLength > 0 {
                field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: retryLength))
            }
            field.typeText(value)
        }
        let final = XCTNSPredicateExpectation(predicate: lengthPredicate, object: field)
        XCTAssertEqual(XCTWaiter.wait(for: [final], timeout: 3), .completed)
    }
    private func signIn(password: String = "fixture-password") {
        text("auth.identifier", "fixture@example.test")
        text("auth.password", password, secure: true)
        tap("auth.submit"); waitLabel("auth.fixture.phase", "signedIn")
        waitLabel("auth.fixture.account", "fixture@example.test")
    }
    private func closeForm() { tap("auth.fixture.close") }

    /// Terminal refresh rejection is injected; authority events, ownership
    /// retirement, and the native shipping sign-in screen are actual code.
    func testTerminalRefreshRejectionRetiresAuthorityAndShowsSignInWithoutAutomaticRetry() {
        launchFixture(expiry: true); signIn()
        waitLabel("auth.fixture.work", "Owned work retained")
        tap("auth.fixture.expire")
        waitLabel("auth.fixture.phase", "signedOut")
        waitLabel("auth.fixture.authority", "Revocations 1; retirements 1; signouts 0")
        waitLabel("auth.fixture.work", "Owned work cleared")
        XCTAssertFalse(app.staticTexts["auth.fixture.account"].exists)
        XCTAssertFalse(app.staticTexts["auth.fixture.revocation-error"].exists)
        XCTAssertTrue(app.textFields["auth.identifier"].isHittable)
        XCTAssertTrue(app.buttons["auth.submit"].exists)
        let capture = XCTAttachment(screenshot: app.screenshot())
        capture.name = "Reference — injected terminal refresh rejection — actual authority observer and sign-in form"
        capture.lifetime = .keepAlways; add(capture)
        signIn()
        waitLabel("auth.fixture.authority", "Revocations 1; retirements 1; signouts 0")
    }

    func testTypedAttestationClockFailureShowsSafeGuidanceAndAllowsExplicitRetry() {
        launchFixture(clockFailure: true)
        text("auth.identifier", "fixture@example.test")
        text("auth.password", "fixture-password", secure: true)
        tap("auth.submit")
        waitLabel(
            "auth.error",
            "This device’s date or time appears incorrect. Check Date & Time and the time zone in Settings, then try again."
        )
        waitLabel("auth.fixture.phase", "signedOut")
        XCTAssertFalse(app.staticTexts["auth.fixture.account"].exists)
        let password = app.secureTextFields["auth.password"]
        let cleared = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                let value = password.value as? String
                return value == "" || value == password.placeholderValue
            }, object: password)
        XCTAssertEqual(XCTWaiter.wait(for: [cleared], timeout: 5), .completed)
        let capture = XCTAttachment(screenshot: app.screenshot())
        capture.name = "Reference — injected typed attestation clock guidance — actual sign-in form"
        capture.lifetime = .keepAlways; add(capture)
        text("auth.password", "fixture-password", secure: true)
        tap("auth.submit")
        waitLabel("auth.fixture.account", "fixture@example.test")
        waitLabel("auth.fixture.method", "email")
        XCTAssertFalse(app.staticTexts["auth.error"].exists)
    }

    func testEmailFormRejectsInvalidCredentialsThenResolvesOwnedAccount() {
        launchFixture()
        text("auth.identifier", "fixture@example.test")
        text("auth.password", "incorrect", secure: true)
        tap("auth.submit")
        XCTAssertTrue(app.staticTexts["auth.error"].waitForExistence(timeout: 10))
        waitLabel("auth.fixture.phase", "signedOut")
        text("auth.password", "fixture-password", secure: true)
        tap("auth.submit"); waitLabel("auth.fixture.account", "fixture@example.test")
        waitLabel("auth.fixture.method", "email")
    }

    func testExistingGuestAndEmailSignupUseActualModeForms() {
        launchFixture(); tap("Use an existing guest account")
        text("auth.identifier", "existing-guest"); text("auth.password", "fixture-password", secure: true)
        tap("auth.submit"); waitLabel("auth.fixture.method", "guest")
        waitLabel("auth.fixture.account", "existing-guest")
        tap("auth.fixture.sign-out"); waitLabel("auth.fixture.phase", "signedOut")
        tap("Create an account")
        text("auth.identifier", "new@example.test"); text("auth.password", "signup-password", secure: true)
        tap("auth.submit"); waitLabel("auth.fixture.account", "new@example.test")
        waitLabel("auth.fixture.method", "email")
    }

    func testPasswordResetRequiresWarningAndOwnedSecretBeforeConfirmation() {
        launchFixture(); tap("auth.forgot")
        text("auth.reset-email", "fixture@example.test")
        tap("auth.request-reset")
        XCTAssertTrue(app.buttons["Continue with reset"].waitForExistence(timeout: 10))
        tap("Continue with reset")
        text("auth.reset-code", "ABCD1234")
        text("auth.reset-password", "reset-password", secure: true)
        text("auth.reset-confirm-password", "reset-password", secure: true)
        tap("auth.confirm-reset")
        XCTAssertTrue(app.buttons["Return to sign-in"].waitForExistence(timeout: 10))
        closeForm()
        waitLabel("auth.fixture.resets", "Reset requests 1; confirmations 1")
        signIn(password: "reset-password")
    }

    func testVerificationAndPasswordChangeKeepTheirAccountScope() {
        launchFixture(); signIn()
        tap("auth.fixture.verify"); tap("auth.verify-resend")
        text("auth.verify-code", "11111111-2222-4333-8444-555555555555")
        tap("auth.verify-submit")
        XCTAssertTrue(app.staticTexts["auth.verified"].waitForExistence(timeout: 10)); closeForm()
        tap("auth.fixture.change")
        text("auth.current-password", "wrong-password", secure: true)
        text("auth.new-password", "changed-password", secure: true)
        text("auth.confirm-new-password", "changed-password", secure: true)
        tap("auth.change-password")
        XCTAssertTrue(app.staticTexts["auth.ceremony-error"].waitForExistence(timeout: 10)); closeForm()
        tap("auth.fixture.change")
        text("auth.current-password", "fixture-password", secure: true)
        text("auth.new-password", "changed-password", secure: true)
        text("auth.confirm-new-password", "changed-password", secure: true)
        tap("auth.change-password")
        XCTAssertTrue(app.staticTexts["auth.password-changed"].waitForExistence(timeout: 10)); closeForm()
        tap("auth.fixture.sign-out"); waitLabel("auth.fixture.phase", "signedOut")
        signIn(password: "changed-password")
    }

    func testDeletionRequiresTypedIntentCodeAndQuiescenceBeforeCleanup() {
        launchFixture(); signIn(); tap("auth.fixture.delete")
        XCTAssertFalse(app.buttons["auth.request-delete"].isEnabled)
        text("auth.delete-confirmation", "DELETE"); tap("auth.request-delete")
        text("auth.delete-code", "11111111-2222-4333-8444-555555555555")
        tap("auth.confirm-delete")
        XCTAssertTrue(app.staticTexts["auth.account-deleted"].waitForExistence(timeout: 10)); closeForm()
        waitLabel("auth.fixture.phase", "signedOut")
        waitLabel("auth.fixture.deletions", "Deletion fences 1; cleanups 1")
    }

    func testDuplicateBrowserDeliveriesRedeemOnceAndRequireExplicitAccountConfirmation() {
        launchFixture(); tap("auth.google")
        waitLabel("auth.fixture.phase", "authenticating")
        waitLabel("auth.fixture.request", "Local request valid")
        tap("auth.fixture.callback-twice")
        waitLabel("auth.fixture.redemptions", "Redemptions 1")
        waitLabel("auth.fixture.installs", "Installs 0")
        tap("auth.fixture.release")
        XCTAssertTrue(app.buttons["auth.confirm-account"].waitForExistence(timeout: 10))
        waitLabel("auth.fixture.phase", "authenticating")
        waitLabel("auth.fixture.installs", "Installs 0")
        tap("auth.confirm-account")
        waitLabel("auth.fixture.account", "browser@example.test")
        waitLabel("auth.fixture.installs", "Installs 1")
    }

    func testInvalidBrowserCallbackAndLateCancelledAttemptCannotInstall() {
        launchFixture(); tap("auth.google"); tap("auth.fixture.wrong-callback")
        waitLabel("auth.fixture.phase", "signedOut")
        XCTAssertTrue(app.staticTexts["auth.ceremony-error"].waitForExistence(timeout: 10))
        waitLabel("auth.fixture.redemptions", "Redemptions 0")
        tap("auth.github"); waitLabel("auth.fixture.phase", "authenticating")
        tap("auth.fixture.old-callback")
        waitLabel("auth.fixture.redemptions", "Redemptions 0")
        waitLabel("auth.fixture.phase", "authenticating")
        tap("auth.fixture.provider-cancel"); waitLabel("auth.fixture.phase", "signedOut")
        tap("auth.fixture.callback-twice")
        waitLabel("auth.fixture.redemptions", "Redemptions 0")
        waitLabel("auth.fixture.installs", "Installs 0")
    }

    func testCancelDuringBrowserRedemptionPreservesSignedOutStateAndDropsLateResult() {
        launchFixture(); tap("auth.google"); tap("auth.fixture.callback-twice")
        waitLabel("auth.fixture.redemptions", "Redemptions 1")
        tap("auth.cancel-provider"); waitLabel("auth.fixture.phase", "signedOut")
        tap("auth.fixture.release"); tap("auth.fixture.callback-twice")
        waitLabel("auth.fixture.installs", "Installs 0")
        XCTAssertFalse(app.buttons["auth.confirm-account"].exists)
    }
}
