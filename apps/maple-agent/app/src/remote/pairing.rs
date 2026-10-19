//! The enrollment proof is consumed, never retried. OpenSecret admission is
//! checked by the caller before taking a host window's single proof attempt.
use pake_cpace::{CPace, ChannelIdentifier, NoAD, UnverifiedNegotiation};
use rand::{RngCore, rngs::OsRng};
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
pub const WINDOW_DURATION: Duration = Duration::from_secs(300);
const FAILED: &str = "Pairing proof failed. Open a new pairing window and try again.";
const INVALID_CODE: &str = "Enter the eight-character pairing code shown on the other computer.";

/// No Debug/Serialize implementation: the code must not enter logs or storage.
pub struct Code(Zeroizing<[u8; 8]>);

impl Code {
    pub fn generate() -> Self {
        let mut bytes = Zeroizing::new([0; 8]);
        OsRng.fill_bytes(bytes.as_mut());
        for byte in bytes.iter_mut() {
            *byte = ALPHABET[usize::from(*byte & 31)];
        }
        Self(bytes)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.as_bytes();
        let (first, last) = match text.len() {
            8 => text.split_at(4),
            9 if text[4] == b'-' => (&text[..4], &text[5..]),
            _ => return Err(INVALID_CODE.into()),
        };
        if !first.iter().chain(last).all(|b| ALPHABET.contains(b)) {
            return Err(INVALID_CODE.into());
        }
        let mut bytes = Zeroizing::new([0; 8]);
        bytes[..4].copy_from_slice(first);
        bytes[4..].copy_from_slice(last);
        Ok(Self(bytes))
    }

    /// Only the locally initiated pairing UI may display this value.
    pub fn display(&self) -> String {
        let code = std::str::from_utf8(self.0.as_ref()).expect("pairing code is ASCII");
        format!("{}-{}", &code[..4], &code[4..])
    }
}

/// All identities are known locally or from authenticated account state. In
/// particular, B's peer key comes from Iroh, not the admission response.
pub struct Context<'a> {
    pub controller: &'a [u8; 32],
    pub execution: &'a [u8; 32],
    pub account: &'a str,
    pub project: &'a str,
    pub deployment: &'a str,
    pub channel: &'a str,
    pub window: &'a [u8; 16],
}

fn start(code: Code, context: &Context<'_>) -> Result<CPace, String> {
    // The library length-prefixes each item and binds it into the generator.
    // The fixed domain pins both roles, protocol and the exact CPace variant.
    let extra_context: &[&[u8]] = &[
        b"maple.remote.pairing.v1/cpace-ristretto255-draft06/controller-to-execution",
        context.account.as_bytes(),
        context.project.as_bytes(),
        context.deployment.as_bytes(),
        context.channel.as_bytes(),
        context.window,
    ];
    CPace::new(
        context.window,
        code.0.as_ref(),
        &ChannelIdentifier {
            id_a: context.controller,
            id_b: context.execution,
            extra_context,
        },
        &NoAD {},
    )
    .map_err(|_| "Could not obtain randomness for pairing".into())
}

pub struct Window {
    pub id: [u8; 16],
    deadline: Instant,
    code: Option<Code>,
}

impl Window {
    pub fn new() -> Self {
        Self {
            id: *uuid::Uuid::new_v4().as_bytes(),
            deadline: Instant::now() + WINDOW_DURATION,
            code: Some(Code::generate()),
        }
    }

    pub fn code(&self) -> Option<String> {
        self.code.as_ref().map(Code::display)
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn is_open(&self, window: &[u8; 16]) -> bool {
        &self.id == window && Instant::now() < self.deadline && self.code.is_some()
    }

    /// Call under the window mutex AFTER admission validation, rechecking the
    /// exact window and account generation there. Taking closes it even if the
    /// subsequent exchange is malformed, cancelled, or disconnected.
    pub fn take_attempt(&mut self, window: &[u8; 16]) -> Result<Code, String> {
        if !self.is_open(window) {
            return Err("Pairing window is closed or already used.".into());
        }
        self.code.take().ok_or_else(|| FAILED.into())
    }
}

/// A sends its public value once, then waits for B's confirmation. Consuming
/// this object prevents a UI retry from testing another host proof on the code.
pub struct Controller(CPace);
impl Controller {
    pub fn new(code: Code, context: &Context<'_>) -> Result<Self, String> {
        start(code, context).map(Self)
    }

    pub fn message(&self) -> &[u8] {
        self.0.message()
    }

    /// No controller confirmation is exposed until B's proof has verified.
    pub fn confirm(self, message: Vec<u8>, host_tag: &[u8]) -> Result<[u8; 64], String> {
        if message == self.0.message() {
            return Err(FAILED.into());
        }
        let (proof, _) = self
            .0
            .initiator_process_message::<NoAD>(message)
            .map_err(|_| FAILED)?;
        let mut tag = proof.tag();
        match proof.verify_tag(host_tag) {
            Ok(mut key) => {
                key.zeroize(); // Pairing pins Iroh identities; no second tunnel.
                Ok(tag)
            }
            Err(_) => {
                tag.zeroize();
                Err(FAILED.into())
            }
        }
    }
}

pub struct Execution(UnverifiedNegotiation);
impl Execution {
    pub fn new(code: Code, context: &Context<'_>, message: Vec<u8>) -> Result<Self, String> {
        let (processed, _) =
            CPace::responder_process_message::<NoAD>(message).map_err(|_| FAILED)?;
        let proof = processed
            .finish(start(code, context)?)
            .map_err(|_| FAILED)?;
        if proof.own_message() == proof.other_message() {
            return Err(FAILED.into());
        }
        Ok(Self(proof))
    }

    pub fn message(&self) -> &[u8] {
        self.0.own_message()
    }

    pub fn confirmation(&self) -> [u8; 64] {
        self.0.tag()
    }

    pub fn verify(self, tag: &[u8]) -> Result<(), String> {
        let mut key = self.0.verify_tag(tag).map_err(|_| FAILED)?;
        key.zeroize();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> Context<'static> {
        Context {
            controller: &[1; 32],
            execution: &[2; 32],
            account: "account",
            project: "project",
            deployment: "local",
            channel: "unpackaged",
            window: &[3; 16],
        }
    }
    fn code() -> Code {
        Code::parse("ABCDEFG2").unwrap()
    }

    #[test]
    fn correct_proofs_confirm_execution_before_controller() {
        let ctx = context();
        let a = Controller::new(Code::parse("ABCD-EFG2").unwrap(), &ctx).unwrap();
        let b = Execution::new(code(), &ctx, a.message().to_vec()).unwrap();
        let tag = a.confirm(b.message().to_vec(), &b.confirmation()).unwrap();
        b.verify(&tag).unwrap();
    }

    #[test]
    fn display_groups_code_without_changing_its_value() {
        assert_eq!(code().display(), "ABCD-EFG2");
        assert_eq!(Code::parse(&code().display()).unwrap().0, code().0);
        for invalid in [
            "ABC-DEFG2",
            "ABCD--EFG2",
            "ABCD-EFG",
            "ABCD-EFG22",
            "ABCD-EFG1",
            "ABCD–EFG2",
        ] {
            assert!(Code::parse(invalid).is_err());
        }
    }

    #[test]
    fn mismatched_code_or_each_bound_identity_fails() {
        for change in 0..8 {
            let ctx = context();
            let a = Controller::new(code(), &ctx).unwrap();
            let mut other = context();
            let mut b_code = code();
            match change {
                0 => other.controller = &[4; 32],
                1 => other.execution = &[4; 32],
                2 => other.account = "other",
                3 => other.project = "other",
                4 => other.deployment = "other",
                5 => other.window = &[4; 16],
                6 => other.channel = "other",
                _ => b_code = Code::parse("ABCDEFG3").unwrap(),
            }
            let b = Execution::new(b_code, &other, a.message().to_vec()).unwrap();
            assert!(a.confirm(b.message().to_vec(), &b.confirmation()).is_err());
        }
    }

    #[test]
    fn malformed_group_elements_tags_and_reflection_fail() {
        for message in [vec![], vec![0; 32], vec![255; 32], vec![1; 31], vec![1; 33]] {
            assert!(Execution::new(code(), &context(), message.clone()).is_err());
            let a = Controller::new(code(), &context()).unwrap();
            assert!(a.confirm(message, &[0; 64]).is_err());
        }
        for tag in [vec![], vec![0; 63], vec![0; 64], vec![0; 65]] {
            let a = Controller::new(code(), &context()).unwrap();
            let b = Execution::new(code(), &context(), a.message().to_vec()).unwrap();
            assert!(a.confirm(b.message().to_vec(), &tag).is_err());
        }
        let a = Controller::new(code(), &context()).unwrap();
        let reflected = a.message().to_vec();
        assert!(a.confirm(reflected, &[0; 64]).is_err());
    }

    #[test]
    fn window_cannot_be_reused_or_extended_by_admission() {
        let mut window = Window::new();
        assert_eq!(window.code().unwrap().len(), 9);
        assert!(Code::parse(&window.code().unwrap()).is_ok());
        assert!(window.take_attempt(&[0; 16]).is_err());
        let id = window.id;
        assert!(window.take_attempt(&id).is_ok());
        assert!(window.take_attempt(&id).is_err());
        let mut window = Window::new();
        window.deadline = Instant::now();
        assert!(window.take_attempt(&window.id.clone()).is_err());
    }
}
