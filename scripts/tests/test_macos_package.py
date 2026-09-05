import copy
import datetime
import hashlib
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("package_macos", Path(__file__).parents[1] / "package-macos.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)

class DistributionPreflight(unittest.TestCase):
    def setUp(self):
        self.team = "EXAMPLETEAM"
        self.cert = b"test certificate"
        self.now = datetime.datetime(2026, 1, 1)
        self.entitlements = {
            "com.apple.application-identifier": self.team + ".com.slio.git",
            "com.apple.developer.team-identifier": self.team,
            "com.apple.security.app-sandbox": True,
            "com.apple.security.files.user-selected.read-write": True,
            "keychain-access-groups": [self.team + ".com.slio.git"],
        }
        self.profile = {"ExpirationDate": datetime.datetime(2027, 1, 1), "TeamIdentifier": [self.team], "DeveloperCertificates": [self.cert], "Entitlements": copy.deepcopy(self.entitlements)}
    def validate(self):
        package.validate_profile(self.profile, self.entitlements, hashlib.sha1(self.cert).hexdigest(), self.team, self.now)
    def test_matching_distribution_profile(self):
        self.validate()
    def test_wrong_app_certificate_expiry_and_debug_profile_fail(self):
        for field, value in [("ExpirationDate", self.now), ("DeveloperCertificates", [b"different"]), ("ProvisionedDevices", ["device"]), ("TeamIdentifier", ["WRONGTEAM"])]:
            with self.subTest(field=field):
                original = copy.deepcopy(self.profile)
                self.profile[field] = value
                with self.assertRaises(ValueError): self.validate()
                self.profile = original
        self.profile["Entitlements"]["com.apple.application-identifier"] = self.team + ".wrong.app"
        with self.assertRaises(ValueError): self.validate()
    def test_keychain_and_sandbox_must_match(self):
        self.entitlements["keychain-access-groups"] = ["ungranted.group"]
        with self.assertRaises(ValueError): self.validate()
        self.entitlements["keychain-access-groups"] = [self.team + ".com.slio.git"]
        self.entitlements["com.apple.security.app-sandbox"] = False
        with self.assertRaises(ValueError): self.validate()

if __name__ == "__main__": unittest.main()
