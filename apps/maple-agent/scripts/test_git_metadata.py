#!/usr/bin/env python3
"""Exercise the production Git helper with a tiny dependency-free Cargo crate.

Uses the selected Rust toolchain, offline, without compiling Maple or touching
its shared Cargo outputs. Temporary repositories have no remotes or user config.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


HELPER = Path(__file__).resolve().parent.parent / "app/git_metadata.rs"


class GitMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="maple-git-metadata-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.repo = self.root / "repo"
        self.repo.mkdir()
        # Ignore inherited repository/worktree routing, injected configuration,
        # compiler wrappers and production target paths. Keep the selected
        # toolchain and its native linker environment from the component shell.
        self.env = {
            key: value for key, value in os.environ.items()
            if not key.startswith("GIT_") and key not in {
                "CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "CARGO_BUILD_TARGET_DIR",
                "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
            }
        }
        self.env.update(
            GIT_CONFIG_GLOBAL=os.devnull,
            GIT_CONFIG_SYSTEM=os.devnull,
            GIT_CONFIG_NOSYSTEM="1",
            GIT_TERMINAL_PROMPT="0",
            GIT_CEILING_DIRECTORIES=str(self.root),
            GIT_AUTHOR_NAME="Metadata fixture",
            GIT_AUTHOR_EMAIL="fixture@example.invalid",
            GIT_COMMITTER_NAME="Metadata fixture",
            GIT_COMMITTER_EMAIL="fixture@example.invalid",
            CARGO_TARGET_DIR=str(self.root / "target"),
            CARGO_BUILD_BUILD_DIR=str(self.root / "build"),
            CARGO_NET_OFFLINE="true",
            FIXTURE_RUN_LOG=str(self.root / "executions"),
        )
        self.write_fixture(self.repo)

    def command(self, *args, cwd=None, env=None):
        result = subprocess.run(
            args, cwd=cwd or self.repo, env=env or self.env,
            capture_output=True, text=True, timeout=90,
        )
        self.assertEqual(result.returncode, 0, " ".join(args) + "\n" + result.stdout + result.stderr)
        return result.stdout.strip()

    def git(self, *args):
        return self.command("git", "-c", "commit.gpgsign=false", *args)

    def write_fixture(self, repo):
        (repo / "Cargo.toml").write_text(
            '[package]\nname = "git-metadata-fixture"\nversion = "0.0.0"\n'
            'edition = "2024"\n[workspace]\n', encoding="utf-8",
        )
        (repo / "Cargo.lock").write_text(
            'version = 4\n[[package]]\nname = "git-metadata-fixture"\nversion = "0.0.0"\n',
            encoding="utf-8",
        )
        (repo / "build.rs").write_text(
            '#[path = ' + json.dumps(HELPER.as_posix()) + ']\nmod git_metadata;\n'
            'fn main() {\n'
            '    let info = git_metadata::emit();\n'
            '    use std::io::Write;\n'
            '    let mut log = std::fs::OpenOptions::new().create(true).append(true)\n'
            '        .open(std::env::var_os("FIXTURE_RUN_LOG").unwrap()).unwrap();\n'
            '    writeln!(log, "{} {}", info.revision, info.source_sha).unwrap();\n'
            '}\n', encoding="utf-8",
        )
        (repo / "src").mkdir()
        (repo / "src/main.rs").write_text(
            'fn main() {\n'
            '    println!("{} {}", env!("MAPLE_GIT_HASH"), env!("MAPLE_GIT_SOURCE_SHA"));\n'
            '}\n'
            '#[test]\nfn metadata_matches_checkout() {\n'
            '    assert_eq!(env!("MAPLE_GIT_HASH"), std::env::var("EXPECTED_GIT_HASH").unwrap());\n'
            '    assert_eq!(env!("MAPLE_GIT_SOURCE_SHA"), std::env::var("EXPECTED_GIT_SHA").unwrap());\n'
            '}\n', encoding="utf-8",
        )
        (repo / "tracked.txt").write_text("first\n", encoding="utf-8")

    def initialize(self):
        self.git("init", "--quiet", "--initial-branch=main", "--object-format=sha1")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "fixture")

    def commit(self, value):
        (self.repo / "tracked.txt").write_text(value + "\n", encoding="utf-8")
        self.git("add", "tracked.txt")
        self.git("commit", "--quiet", "-m", value)

    def expected(self, *, dirty=False, no_git=False):
        if no_git:
            return "unknown", "unknown"
        short = self.git("rev-parse", "--short", "HEAD")
        return short + ("-dirty" if dirty else ""), self.git("rev-parse", "HEAD")

    def count(self):
        log = self.root / "executions"
        return len(log.read_text(encoding="utf-8").splitlines()) if log.exists() else 0

    def cargo(self, command, expected, *, fresh=False):
        env = dict(self.env, EXPECTED_GIT_HASH=expected[0], EXPECTED_GIT_SHA=expected[1])
        output = self.command("cargo", command, "--offline", "--locked", "--message-format=json", env=env)
        artifacts = []
        for line in output.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("reason") == "compiler-artifact":
                artifacts.append(event)
        self.assertTrue(artifacts, output)
        if fresh:
            self.assertTrue(all(event["fresh"] for event in artifacts), output)
        if command == "build":
            binary = Path(self.env["CARGO_TARGET_DIR"]) / "debug/git-metadata-fixture"
            if os.name == "nt":
                binary = binary.with_suffix(".exe")
            self.assertEqual(self.command(str(binary)), " ".join(expected))
        self.assertEqual((self.root / "executions").read_text(encoding="utf-8").splitlines()[-1],
                         " ".join(expected))

    def build_and_test(self, expected, executions):
        self.cargo("build", expected)
        self.assertEqual(self.count(), executions)
        self.cargo("test", expected)
        self.assertEqual(self.count(), executions, "build → test reran the metadata build script")
        self.cargo("test", expected, fresh=True)
        self.assertEqual(self.count(), executions)
        self.cargo("build", expected, fresh=True)
        self.assertEqual(self.count(), executions)

    def test_loose_branch_is_fresh_and_new_commit_refreshes_both_hashes(self):
        self.initialize()
        self.assertFalse((self.repo / ".git/packed-refs").exists())
        self.build_and_test(self.expected(), 1)
        self.commit("second")
        self.build_and_test(self.expected(), 2)

    def test_detached_head_without_packed_refs_is_fresh_and_tracks_checkout(self):
        self.initialize()
        first = self.git("rev-parse", "HEAD")
        self.commit("second")
        self.git("checkout", "--quiet", "--detach", first)
        self.assertFalse((self.repo / ".git/packed-refs").exists())
        self.build_and_test(self.expected(), 1)
        self.git("checkout", "--quiet", "--detach", "main")
        self.build_and_test(self.expected(), 2)

    def test_packed_branch_stays_fresh_and_packed_to_loose_commit_refreshes(self):
        self.initialize()
        self.build_and_test(self.expected(), 1)
        self.git("pack-refs", "--all", "--prune")
        self.assertFalse((self.repo / ".git/refs/heads/main").exists())
        self.build_and_test(self.expected(), 2)
        self.commit("after packing")
        self.assertTrue((self.repo / ".git/refs/heads/main").exists())
        self.build_and_test(self.expected(), 3)

    def test_missing_nested_ref_parent_creation_and_branch_switches(self):
        self.initialize()
        self.git("checkout", "--quiet", "-b", "feature/nested/topic")
        self.git("pack-refs", "--all", "--prune")
        # Git may retain empty ref directories after packing. Remove them to
        # exercise the nearest-existing-ancestor fallback at multiple levels.
        nested = self.repo / ".git/refs/heads/feature/nested"
        for directory in (nested, nested.parent):
            if directory.exists():
                directory.rmdir()
        self.build_and_test(self.expected(), 1)
        self.commit("nested loose ref")
        self.build_and_test(self.expected(), 2)
        self.git("checkout", "--quiet", "main")
        self.build_and_test(self.expected(), 3)
        self.git("checkout", "--quiet", "feature/nested/topic")
        self.build_and_test(self.expected(), 4)

    def test_packed_ref_update_refreshes_without_a_loose_ref_or_head_change(self):
        self.initialize()
        first = self.git("rev-parse", "HEAD")
        self.git("commit", "--quiet", "--allow-empty", "-m", "same tree, different revision")
        second = self.git("rev-parse", "HEAD")
        self.git("pack-refs", "--all", "--prune")
        self.build_and_test(self.expected(), 1)
        packed = self.repo / ".git/packed-refs"
        contents = packed.read_bytes()
        old_pointer = f"{second} refs/heads/main".encode("ascii")
        new_pointer = f"{first} refs/heads/main".encode("ascii")
        self.assertEqual(contents.count(old_pointer), 1)
        # Move the packed pointer between existing objects without touching
        # HEAD or the loose-ref directory, isolating the packed-file input.
        # Git metadata uses LF on every platform. Preserve its bytes rather
        # than translating newlines to CRLF through Windows text-mode IO.
        packed.write_bytes(contents.replace(old_pointer, new_pointer))
        self.assertFalse((self.repo / ".git/refs/heads/main").exists())
        self.build_and_test(self.expected(), 2)

    def test_linked_worktree_uses_private_head_and_common_refs(self):
        self.initialize()
        self.git("worktree", "add", "--quiet", "-b", "linked/nested", str(self.root / "linked"))
        self.repo = self.root / "linked"
        self.assertTrue((self.repo / ".git").is_file())
        self.build_and_test(self.expected(), 1)
        self.git("pack-refs", "--all", "--prune")
        self.build_and_test(self.expected(), 2)
        self.commit("linked packed to loose")
        self.build_and_test(self.expected(), 3)
        self.git("checkout", "--quiet", "--detach", "HEAD~")
        self.build_and_test(self.expected(), 4)

    def test_symbolic_branch_alias_chain_tracks_commits_and_retargets(self):
        self.initialize()
        self.git("branch", "other")
        self.git("symbolic-ref", "refs/heads/alias2", "refs/heads/main")
        self.git("symbolic-ref", "refs/heads/alias1", "refs/heads/alias2")
        self.git("symbolic-ref", "HEAD", "refs/heads/alias1")
        self.build_and_test(self.expected(), 1)
        self.commit("through aliases")
        self.build_and_test(self.expected(), 2)
        self.git("symbolic-ref", "refs/heads/alias2", "refs/heads/other")
        self.git("reset", "--hard", "--quiet")
        self.build_and_test(self.expected(), 3)

    def test_dirty_suffix_refreshes_only_with_git_pointer_events(self):
        self.initialize()
        clean = self.expected()
        self.build_and_test(clean, 1)
        (self.repo / "tracked.txt").write_text("dirty working tree\n", encoding="utf-8")
        self.build_and_test(clean, 1)
        self.git("checkout", "--quiet", "-b", "dirty-branch")
        dirty = self.expected(dirty=True)
        self.build_and_test(dirty, 2)
        self.git("checkout", "--", "tracked.txt")
        self.build_and_test(dirty, 2)
        self.git("checkout", "--quiet", "main")
        self.build_and_test(clean, 3)

    def test_source_archive_without_git_emits_unknown_and_stays_fresh(self):
        self.build_and_test(self.expected(no_git=True), 1)


if __name__ == "__main__":
    for tool in ("git", "cargo", "rustc"):
        if shutil.which(tool) is None:
            raise SystemExit(f"{tool} is required; run through the Maple Agent pinned Nix shell")
    unittest.main()
