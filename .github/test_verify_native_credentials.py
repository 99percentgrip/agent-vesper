"""Offline parser/guard tests; these never invoke Cargo or native storage."""

import unittest
from verify_native_credentials import has_exact_receipt, require_hosted_fixture


class NativeReceiptTests(unittest.TestCase):
    def test_accepts_one_exact_pass_with_lf_or_crlf(self):
        output = "test fixture::required ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 9 filtered out; finished in 0.01s\n"
        for text in [output, output.replace("\n", "\r\n")]:
            self.assertTrue(has_exact_receipt(text, "fixture::required"))

    def test_missing_renamed_ignored_failed_and_zero_matches_are_not_proof(self):
        for text in [
            "test result: ok. 0 passed; 0 failed; 0 ignored; 2 filtered out;\n",
            "test fixture::renamed ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n",
            "test fixture::required ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored;\n",
            "test fixture::required ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored;\n",
            "test fixture::required ... ok\n",
        ]:
            self.assertFalse(has_exact_receipt(text, "fixture::required"))

    def test_hosted_scope_requires_all_explicit_guards(self):
        allowed = {
            "GITHUB_ACTIONS": "true", "RUNNER_OS": "Windows",
            "VESPER_NATIVE_CREDENTIAL_ACCEPTANCE": "isolated-hosted-fixture",
        }
        require_hosted_fixture(allowed)
        for omitted in allowed:
            with self.assertRaises(RuntimeError):
                require_hosted_fixture({k: v for k, v in allowed.items() if k != omitted})
        with self.assertRaises(RuntimeError):
            require_hosted_fixture({})


if __name__ == "__main__":
    unittest.main()
