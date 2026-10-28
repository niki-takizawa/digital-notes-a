"""JSON Schema Draft 2020-12 contract checks (requires jsonschema)."""

from copy import deepcopy
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]
ENTITIES = ("work", "asset", "card")


def load_json(path: Path):
    with path.open(encoding="utf-8") as stream:
        return json.load(stream)


class SchemaContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schemas = {
            name: load_json(ROOT / "schemas" / f"{name}.schema.json")
            for name in ENTITIES
        }
        cls.fixtures = {
            name: load_json(ROOT / "tests" / "fixtures" / f"{name}.json")
            for name in ENTITIES
        }
        # Explicit format checking is necessary; format is advisory by default.
        cls.validators = {
            name: Draft202012Validator(schema, format_checker=FormatChecker())
            for name, schema in cls.schemas.items()
        }

    def assert_invalid(self, name, obj, case):
        with self.subTest(entity=name, case=case):
            self.assertFalse(self.validators[name].is_valid(obj))

    def test_schemas_are_valid_draft_2020_12(self):
        for name, schema in self.schemas.items():
            with self.subTest(entity=name):
                self.assertEqual(
                    schema["$schema"], "https://json-schema.org/draft/2020-12/schema"
                )
                Draft202012Validator.check_schema(schema)

    def test_fixtures_match_schema_and_field_surface(self):
        for name, fixture in self.fixtures.items():
            with self.subTest(entity=name):
                schema = self.schemas[name]
                self.validators[name].validate(fixture)
                self.assertEqual(set(fixture), set(schema["properties"]))
                self.assertEqual(set(schema["required"]), set(schema["properties"]))
                self.assertIs(schema["additionalProperties"], False)

    def test_every_top_level_field_is_required_and_unknowns_fail(self):
        for name, fixture in self.fixtures.items():
            for key in fixture:
                invalid = deepcopy(fixture)
                del invalid[key]
                self.assert_invalid(name, invalid, f"missing {key}")
            invalid = deepcopy(fixture)
            invalid["unexpected_field"] = True
            self.assert_invalid(name, invalid, "extra field")

    def test_date_time_format_checker_is_active(self):
        checker = FormatChecker()
        self.assertTrue(checker.conforms("2026-02-28T10:00:00Z", "date-time"))
        self.assertFalse(
            checker.conforms("2026-02-30T10:00:00Z", "date-time"),
            "RFC 3339 date-time validation is inactive; install requirements-schema.txt",
        )

    def test_shared_primitive_rejections(self):
        for name, fixture in self.fixtures.items():
            for field, value in (
                ("id", "short"),
                ("title", " \t "),
                ("created", "2026-02-30T10:00:00Z"),
                ("updated", "2026-10-28T10:00:00+00:00"),
                ("schema_version", 2),
                ("entity_type", "other"),
            ):
                invalid = deepcopy(fixture)
                invalid[field] = value
                self.assert_invalid(name, invalid, f"{field}={value!r}")

    def test_work_and_asset_specific_constraints(self):
        for name, changes in (
            ("work", {"status": "paused", "work_type": "NotSnakeCase"}),
            ("asset", {
                "url": "ftp://example.org/example",
                "file_format": "pdf",
                "capture_method": "SingleFile",
                "content_hash": "BAD_HASH",
                "hash_algorithm": "sha1",
            }),
        ):
            for field, value in changes.items():
                invalid = deepcopy(self.fixtures[name])
                invalid[field] = value
                self.assert_invalid(name, invalid, field)

    def test_work_asset_and_card_references_accept_multiple_ids(self):
        for name, field in (
            ("work", "source_work_ids"),
            ("asset", "related_work_ids"),
            ("card", "source_asset_ids"),
            ("card", "source_card_ids"),
        ):
            valid = deepcopy(self.fixtures[name])
            valid[field] = ["reference1", "reference2"]
            with self.subTest(entity=name, field=field):
                self.validators[name].validate(valid)
            invalid = deepcopy(valid)
            invalid[field] = ["reference1", "reference1"]
            self.assert_invalid(name, invalid, f"duplicate {field}")

    def test_nested_evidence_fields_and_constraints(self):
        name = "card"
        schema = self.schemas[name]["$defs"]["evidence"]
        evidence = self.fixtures[name]["evidence"][0]
        self.assertEqual(set(evidence), set(schema["properties"]))
        self.assertEqual(set(schema["required"]), set(schema["properties"]))
        for key in evidence:
            invalid = deepcopy(self.fixtures[name])
            del invalid["evidence"][0][key]
            self.assert_invalid(name, invalid, f"missing evidence.{key}")

        for nested in ("selector", "position_selector"):
            definition = schema["properties"][nested]
            self.assertEqual(set(evidence[nested]), set(definition["properties"]))
            self.assertEqual(
                set(definition["required"]), set(definition["properties"])
            )
            for key in evidence[nested]:
                invalid = deepcopy(self.fixtures[name])
                del invalid["evidence"][0][nested][key]
                self.assert_invalid(name, invalid, f"missing {nested}.{key}")

        for pointer, replacement in (
            (("id",), "ev0"),
            (("text_normalization",), "plain_text"),
            (("selector", "type"), "OtherSelector"),
            (("selector", "exact"), ""),
            (("position_selector", "start"), -1),
            (("position_selector", "end"), 0),
        ):
            invalid = deepcopy(self.fixtures[name])
            target = invalid["evidence"][0]
            for key in pointer[:-1]:
                target = target[key]
            target[pointer[-1]] = replacement
            self.assert_invalid(name, invalid, ".".join(pointer))

        invalid = deepcopy(self.fixtures[name])
        invalid["evidence"][0]["selector"]["unknown"] = 1
        self.assert_invalid(name, invalid, "unknown selector field")


if __name__ == "__main__":
    unittest.main()
