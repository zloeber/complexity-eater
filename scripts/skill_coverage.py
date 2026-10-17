#!/usr/bin/env python3
"""Check Complexity Eater skill metadata, coverage, and description routing."""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COVERAGE = ROOT / "skills" / "coverage.v1.json"
FRONT = re.compile(r"\A---\n(.*?)\n---\n", re.S)
PLACEHOLDER = re.compile(r"\{\{[^{}]+\}\}")


def fail(message):
    print(f"skill coverage: {message}", file=sys.stderr)
    sys.exit(1)


def parse_skill(path):
    text = path.read_text()
    match = FRONT.search(text)
    if not match:
        return None, text, "missing front matter"
    meta = {}
    for line in match.group(1).splitlines():
        if not line.strip():
            continue
        if ":" not in line:
            return None, text, f"front matter line has no key: {line}"
        key, value = line.split(":", 1)
        key = key.strip()
        if key in meta:
            return None, text, f"duplicate front matter key {key}"
        meta[key] = value.strip()
    return meta, text, None


def signals_hit(utterance, signals):
    text = utterance.casefold()
    return [signal for signal in signals if signal.casefold() in text]


def route(utterance, skills):
    chosen = []
    for skill in skills:
        if signals_hit(utterance, skill["signals"]):
            chosen.append(skill["name"])
    return chosen


def lint(coverage, parsed, repo):
    errors = []
    skills = coverage["skills"]
    names = [skill["name"] for skill in skills]
    if len(names) != len(set(names)):
        errors.append("skill names are not unique")
    by_name = {}
    limit = coverage["description_max_chars"]
    for skill in skills:
        path = repo / skill["file"]
        meta, body, problem = parsed.get(skill["name"], (None, "", "skill was not parsed"))
        if problem:
            errors.append(f"{skill['name']}: {problem}")
            continue
        if not path.is_file():
            errors.append(f"{skill['name']}: missing {skill['file']}")
            if not meta:
                continue
        keys = set(meta)
        if keys != {"name", "description"}:
            errors.append(f"{skill['name']}: front matter must be name and description only")
        if meta.get("name") != skill["name"]:
            errors.append(f"{skill['file']}: name is {meta.get('name')}")
        description = meta.get("description", "")
        if not description:
            errors.append(f"{skill['name']}: description is empty")
        if len(description) > limit:
            errors.append(f"{skill['name']}: description is {len(description)} characters")
        if ":" in description:
            errors.append(f"{skill['name']}: description contains a colon")
        for signal in skill["signals"]:
            if signal.casefold() not in description.casefold():
                errors.append(f"{skill['name']}: signal {signal!r} is not in the description")
        for phrase in skill["must_say"]:
            if phrase not in body:
                errors.append(f"{skill['name']}: missing {phrase!r}")
        for section in coverage["required_sections"]:
            if f"## {section}" not in body:
                errors.append(f"{skill['name']}: missing section {section}")
        if skill["publish"] == "render" and PLACEHOLDER.search(body) is None:
            errors.append(f"{skill['name']}: template is missing a release placeholder")
        if skill["publish"] == "copy" and PLACEHOLDER.search(body):
            errors.append(f"{skill['name']}: published copy still has a placeholder")
        by_name[skill["name"]] = skill
        if skill.get("served_by_room"):
            protocol = (repo / "crates" / "ce-protocol" / "src" / "lib.rs").read_text()
            if "skill://commonroom/SKILL.md" not in protocol:
                errors.append("room skill URI is missing from ce-protocol")
            mcp = (repo / "crates" / "ce-http" / "src" / "mcp.rs").read_text()
            if 'skills_list() -> Value' not in mcp or "skill_entry()" not in mcp:
                errors.append("MCP skills list is missing")

    cli = (repo / "crates" / "ce-cli" / "src" / "cli.rs").read_text()
    start = cli.find("const COMMANDS: &[&str] = &[")
    end = cli.find("];", start)
    commands = set(re.findall(r'"([^"]+)"', cli[start:end])) if start >= 0 and end > start else set()
    mcp = (repo / "crates" / "ce-http" / "src" / "mcp.rs").read_text()
    routes = (repo / "crates" / "ce-http" / "src" / "app.rs").read_text()
    routing = {item["utterance"]: item["skills"] for item in coverage["routing"]}
    if len(routing) != len(coverage["routing"]):
        errors.append("routing utterances are not unique")

    for item in coverage["routing"]:
        expected = item["skills"]
        unknown = [name for name in expected if name not in by_name]
        if unknown:
            errors.append(f"{item['utterance']}: unknown skill {unknown}")
            continue
        actual = route(item["utterance"], skills)
        if actual != expected and sorted(actual) != sorted(expected):
            errors.append(f"{item['utterance']}: selected {actual}, expected {expected}")

    for case in coverage["use_cases"]:
        if not case["skills"]:
            errors.append(f"{case['id']}: use case has no skill")
        for name in case["skills"]:
            if name not in by_name:
                errors.append(f"{case['id']}: missing skill {name}")
        for request in case["requests"]:
            if request not in routing:
                errors.append(f"{case['id']}: request is not in the routing table: {request}")
                continue
            selected = set(routing[request])
            allowed = set(case["skills"])
            if not selected or not selected <= allowed:
                errors.append(f"{case['id']}: {request} selected {sorted(selected)}")
        for command in case.get("cli", []):
            if command not in commands:
                errors.append(f"{case['id']}: unknown ce command {command}")
        for tool in case.get("mcp", []):
            if f'"{tool}"' not in mcp:
                errors.append(f"{case['id']}: MCP tool {tool} is missing")
        for path in case.get("http", []):
            if f'"{path}"' not in routes:
                errors.append(f"{case['id']}: HTTP route {path} is missing")
        for uri in case.get("resources", []):
            if uri not in mcp and uri not in (repo / "crates" / "ce-protocol" / "src" / "lib.rs").read_text():
                errors.append(f"{case['id']}: resource {uri} is missing")
        for test in case["tests"]:
            file = repo / test["file"]
            if not file.is_file():
                errors.append(f"{case['id']}: missing test file {test['file']}")
            elif "symbol" in test and test["symbol"] not in file.read_text():
                errors.append(f"{case['id']}: missing symbol {test['symbol']}")
        for doc in case["docs"]:
            if not (repo / doc).is_file():
                errors.append(f"{case['id']}: missing doc {doc}")

    bodies = {skill["name"]: parsed[skill["name"]][1] for skill in skills if skill["name"] in parsed}
    published = "\n".join(bodies.values())
    for item in coverage["optional"]:
        body = bodies.get(item["skill"], "")
        if item["marker"] not in body or item["fallback"] not in body:
            errors.append(f"optional {item['id']}: marker or fallback is missing from {item['skill']}")
    for item in coverage["unsupported"]:
        body = bodies.get(item["skill"], "")
        if item["phrase"] not in body:
            errors.append(f"unsupported {item['id']}: phrase missing from {item['skill']}")
        for forbidden in item["forbidden"]:
            if forbidden in published:
                errors.append(f"unsupported {item['id']}: advertised {forbidden!r}")
    return errors


def prove_failures():
    coverage = {
        "description_max_chars": 80,
        "required_sections": ["When to use"],
        "skills": [
            {
                "name": "gone",
                "file": "skills/missing.md",
                "publish": "copy",
                "served_by_room": False,
                "signals": ["set up"],
                "must_say": ["present"],
            }
        ],
        "use_cases": [
            {
                "id": "orphan",
                "requests": ["Help me get set up."],
                "skills": ["missing-skill"],
                "cli": ["not-a-command"],
                "mcp": ["not_a_tool"],
                "http": ["/missing"],
                "tests": [{"file": "scripts/missing.py", "symbol": "missing"}],
                "docs": ["docs/missing.md"],
            }
        ],
        "routing": [{"utterance": "Help me get set up.", "skills": ["gone"]}],
        "optional": [{"id": "x", "skill": "gone", "marker": "optional-marker", "fallback": "fallback-text"}],
        "unsupported": [
            {
                "id": "hosted",
                "skill": "gone",
                "phrase": "not implemented",
                "forbidden": ["hosted room is ready"],
            }
        ],
    }
    parsed = {
        "gone": (
            {"name": "gone", "description": "no signal here"},
            "hosted room is ready\n## When to use\n",
            None,
        )
    }
    found = lint(coverage, parsed, ROOT)
    needed = [
        "signal",
        "missing-skill",
        "not-a-command",
        "not_a_tool",
        "hosted room is ready",
        "When to use",
    ]
    # The fixture body includes the section, so absence is not required.
    # It must still fail closed on the orphan, the missing signal, and the forbidden claim.
    text = "\n".join(found)
    missing = [item for item in ("signal", "missing-skill", "not-a-command", "not_a_tool", "hosted room is ready") if item not in text]
    if missing:
        fail(f"failure fixture did not report {missing}; saw: {found}")
    if not found:
        fail("failure fixture produced no errors")


def check_dist(dist):
    errors = []
    coverage = json.loads(COVERAGE.read_text())
    for skill in coverage["skills"]:
        source = ROOT / skill["file"]
        if skill["publish"] == "copy":
            published = dist / "skills" / "commonroom" / source.name
            if not published.is_file():
                errors.append(f"site is missing {source.name}")
            elif published.read_bytes() != source.read_bytes():
                errors.append(f"site copy of {source.name} drifted")
        else:
            for relative in ("agent", "agent.md", "skills/commonroom/AGENT.md"):
                page = dist / relative
                if not page.is_file():
                    errors.append(f"site is missing {relative}")
                    continue
                text = page.read_text()
                if PLACEHOLDER.search(text):
                    errors.append(f"{relative} still has a template placeholder")
                if skill["public_url"] not in text and relative == "agent":
                    errors.append("agent entry is missing its canonical URL")
            agent = (dist / "agent").read_text()
            for other in coverage["skills"]:
                if other["public_url"] not in agent:
                    errors.append(f"agent entry is missing {other['public_url']}")
    return errors


def main():
    prove_failures()
    coverage = json.loads(COVERAGE.read_text())
    parsed = {}
    errors = []
    for skill in coverage["skills"]:
        path = ROOT / skill["file"]
        if not path.is_file():
            parsed[skill["name"]] = (None, "", f"missing file {skill['file']}")
            continue
        meta, body, problem = parse_skill(path)
        parsed[skill["name"]] = (meta, body, problem)
    errors.extend(lint(coverage, parsed, ROOT))
    if "--dist" in sys.argv:
        dist = Path(sys.argv[sys.argv.index("--dist") + 1])
        errors.extend(check_dist(dist))
    if errors:
        for error in errors:
            print(f"skill coverage: {error}", file=sys.stderr)
        sys.exit(1)
    print(f"skill coverage: {len(coverage['skills'])} skills, {len(coverage['use_cases'])} use cases, {len(coverage['routing'])} requests")


if __name__ == "__main__":
    main()
