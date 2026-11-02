"""The gate fails closed, splits Rust from Web, and the pin and queue stay consistent."""
from pathlib import Path
import re
import unittest

import bazel_gate
import remote_gate

ROOT = Path(__file__).resolve().parents[2]
TESTS = {'//crates/console:lib_test', '//crates/console:console_test', '//web:unit', '//web:playwright'}
WEB = {'//web:unit', '//web:playwright'}
USED = {'crates/console/src/lib.rs', 'web/src/App.tsx', 'web/package.json'}


def plan(changed, impacted=(), deleted=()):
    return bazel_gate.plan(set(changed), set(deleted), set(impacted), USED, TESTS)


class FakeWorkspace:
    def __init__(self, changed, impacted, statuses, build_ok=True):
        self.changed, self.impacted, self.statuses = changed, impacted, statuses
        self.build_ok, self.ran, self.built = build_ok, [], []

    def ensure_commit(self, sha):
        if not sha:
            raise bazel_gate.GateError('empty base or head')

    def changes(self, base, head):
        return set(self.changed), set()

    def bazel_diff(self, base, head):
        return set(self.impacted)

    def checkout(self, sha):
        pass

    def tests(self):
        return set(TESTS)

    def web_tests(self):
        return set(WEB)

    def used_sources(self):
        return set(USED)

    def test(self, targets, config, metadata, log_name):
        self.ran.append(list(targets))
        return {label: self.statuses.get(label, 'PASSED') for label in targets}

    def build(self, targets, config, metadata, log_name):
        self.built.append(list(targets))
        return self.build_ok


class Selection(unittest.TestCase):
    def test_an_empty_diff_fails(self):
        with self.assertRaises(bazel_gate.GateError):
            plan([])

    def test_toolchain_and_lock_changes_test_everything(self):
        for path in ['Cargo.lock', 'MODULE.bazel', 'web/package-lock.json', '.github/workflows/gate.yml',
                     'tools/bazel/tl3_source.bzl']:
            decision = plan([path])
            self.assertEqual(decision['selection'], 'fallback', path)
            self.assertEqual(decision['selected'], sorted(TESTS))
            self.assertTrue(decision['all_scopes'])

    def test_a_file_no_target_reads_fails(self):
        with self.assertRaisesRegex(bazel_gate.GateError, 'deploy/new.py'):
            plan(['deploy/new.py'], impacted=['//web:unit'])

    def test_docs_alone_select_nothing(self):
        decision = plan(['README.md', 'docs/notes.md'])
        self.assertEqual((decision['selection'], decision['selected']), ('irrelevant', []))

    def test_impacted_tests_and_packages_become_the_selection_and_scopes(self):
        decision = plan(['web/src/App.tsx'], impacted=['//web:unit', '//web:sources'])
        self.assertEqual(decision['selected'], ['//web:unit'])
        self.assertEqual(decision['scopes'], ['//web'])
        self.assertEqual(decision['build'], ['//web:sources'])

    def test_a_relevant_change_that_impacts_nothing_fails(self):
        with self.assertRaises(bazel_gate.GateError):
            plan(['crates/console/src/lib.rs'])


class Verdicts(unittest.TestCase):
    def test_web_failures_fail_only_web(self):
        verdicts, failed = bazel_gate.classify(sorted(TESTS), {'//web:playwright': 'FAILED'} | {
            label: 'PASSED' for label in TESTS - {'//web:playwright'}}, WEB)
        self.assertEqual(verdicts, {'rust': 'pass', 'web': 'fail'})
        self.assertEqual(failed['web'], ['//web:playwright'])

    def test_a_missing_status_fails(self):
        verdicts, _ = bazel_gate.classify(['//crates/console:lib_test'], {}, WEB)
        self.assertEqual(verdicts['rust'], 'fail')

    def test_an_unselected_language_is_skipped(self):
        verdicts, _ = bazel_gate.classify(['//web:unit'], {'//web:unit': 'PASSED'}, WEB)
        self.assertEqual(verdicts, {'rust': 'skipped', 'web': 'pass'})

    def test_pr_gate_reports_a_failed_build_as_an_input_problem(self):
        workspace = FakeWorkspace(['web/src/App.tsx'], ['//web:unit', '//web:sources'], {}, build_ok=False)
        result = bazel_gate.gate_pr(workspace, 'a' * 40, 'b' * 40, {})
        self.assertEqual(workspace.ran, [['//web:unit']])
        self.assertTrue(result['inputs'])
        self.assertFalse(bazel_gate.passed({**result, 'mode': 'pr', 'error': None}))

    def test_main_runs_everything_and_names_detection_misses(self):
        workspace = FakeWorkspace(['web/src/App.tsx'], ['//web:unit'], {'//crates/console:lib_test': 'FAILED'})
        result = bazel_gate.gate_main(workspace, 'a' * 40, 'b' * 40, {})
        self.assertEqual(workspace.ran, [sorted(TESTS)])
        self.assertEqual(result['architectures']['amd64']['missed'], ['//crates/console:lib_test'])


class Checks(unittest.TestCase):
    def needs(self, rust='success', web='success', uploaded='true', **jobs):
        return {'bazel': {'result': 'success', 'outputs': {'rust': rust, 'web': web,
                                                           'scopes_uploaded': uploaded}},
                'rust-lint': {'result': jobs.get('lint', 'success')}}

    def test_each_check_needs_its_own_verdict(self):
        remote_gate.aggregate('rust', self.needs(web='failure'))
        with self.assertRaises(RuntimeError):
            remote_gate.aggregate('web', self.needs(web='failure'))

    def test_checks_need_scopes_and_every_other_job(self):
        with self.assertRaises(RuntimeError):
            remote_gate.aggregate('web', self.needs(uploaded='false'))
        with self.assertRaises(RuntimeError):
            remote_gate.aggregate('web', self.needs(lint='failure'))

    def test_outputs_fail_both_languages_on_detection_errors(self):
        result = {'mode': 'pr', 'error': 'no base', 'verdicts': {'rust': 'pass', 'web': 'pass'}}
        self.assertEqual(remote_gate.outputs(result)['rust'], 'failure')
        self.assertEqual(remote_gate.outputs(result)['web'], 'failure')
        self.assertEqual(remote_gate.outputs(result)['scopes_ready'], 'false')


class Configuration(unittest.TestCase):
    def test_cargo_pins_tl3_at_one_full_commit(self):
        manifest = (ROOT / 'Cargo.toml').read_text()
        lock = (ROOT / 'Cargo.lock').read_text()
        pinned = set(re.findall(r'trustladderiii", rev = "([0-9a-f]+)"', manifest))
        locked = set(re.findall(r'trustladderiii\?rev=[0-9a-f]+#([0-9a-f]+)"', lock))
        self.assertEqual(len(pinned), 1)
        self.assertEqual(pinned, locked)
        self.assertEqual(len(next(iter(pinned))), 40)

    def test_the_queue_merges_on_rust_and_web(self):
        mergify = (ROOT / '.mergify.yml').read_text()
        rules = mergify.split('\nqueue_rules:\n')[1].split('\npull_request_rules:\n')[0]
        for rule in rules.split('  - name: ')[1:]:
            conditions = rule.split('merge_conditions:')[1].split('merge_method')[0]
            self.assertIn('check-success = Rust', conditions)
            self.assertIn('check-success = Web', conditions)

    def test_workflow_names_the_required_checks(self):
        workflow = (ROOT / '.github/workflows/gate.yml').read_text()
        self.assertIn('    name: Rust\n', workflow)
        self.assertIn('    name: Web\n', workflow)


if __name__ == '__main__':
    unittest.main()
