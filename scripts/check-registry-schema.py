"""Validate the published JSON Schema and inventory without fetching references.

Install scripts/requirements-registry.txt first. Semantic implementation/review
checks belong to the Rust connector-registry command.
"""
import json
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker


def check_local_refs(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key == '$ref' and not child.startswith('#/'):
                raise ValueError('Registry schema permits only local $ref values')
            check_local_refs(child)
    elif isinstance(value, list):
        for child in value:
            check_local_refs(child)


def main():
    base = Path(__file__).resolve().parents[1] / 'docs/connectors'
    schema = json.loads((base / 'registry.schema.json').read_text())
    check_local_refs(schema)
    Draft202012Validator.check_schema(schema)
    registry = json.loads((base / 'registry.json').read_text())
    Draft202012Validator(schema, format_checker=FormatChecker()).validate(registry)
    print('PASS: connector inventory JSON Schema 2 and date/URI formats')


if __name__ == '__main__':
    main()
