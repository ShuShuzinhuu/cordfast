#!/usr/bin/env python3
"""Keeps the interface's catalogs (assets/i18n/*.po) in step with the code.

Collects every string the code translates: t!("…"), tn!(n, "…", "…"),
n!("…"), tp!("context", "…") and tr("…") / ntr("…", "…") with literals.
Then, for each catalog,
writes the messages in the code's order, keeping the translations it has
and leaving new ones empty (they show in English until translated).

    scripts/i18n.py                       update every catalog
    scripts/i18n.py --check               fail if a catalog misses messages
    scripts/i18n.py --import pt-BR f.json fill translations from a JSON map
                                          (English -> text, or a list of
                                          forms for a plural; "context\u0004
                                          English" for a tp!)
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CATALOGS = ROOT / "assets" / "i18n"

HEADERS = {
    "pt-BR": ("Portuguese (Brazil)", "nplurals=2; plural=(n > 1);"),
    "es-ES": ("Spanish (Spain)", "nplurals=2; plural=(n != 1);"),
    "fr": ("French", "nplurals=2; plural=(n > 1);"),
    "de": ("German", "nplurals=2; plural=(n != 1);"),
    "it": ("Italian", "nplurals=2; plural=(n != 1);"),
    "ru": ("Russian", "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);"),
    "ja": ("Japanese", "nplurals=1; plural=0;"),
}

LITERAL = r'"((?:[^"\\]|\\.)*)"'
PATTERNS = [
    (re.compile(r"\bt!\(\s*" + LITERAL), "one"),
    (re.compile(r"\bn!\(\s*" + LITERAL), "one"),
    (re.compile(r"\btr\(\s*" + LITERAL), "one"),
    (re.compile(r"\btn!\([^,]+,\s*" + LITERAL + r"\s*,\s*" + LITERAL, re.S), "plural"),
    (re.compile(r"\bntr\(\s*" + LITERAL + r"\s*,\s*" + LITERAL, re.S), "plural"),
    (re.compile(r"\btp!\(\s*" + LITERAL + r"\s*,\s*" + LITERAL, re.S), "context"),
]


def unescape(text):
    """A Rust string literal's contents as the string it is."""
    out, i = [], 0
    while i < len(text):
        c = text[i]
        if c == "\\" and i + 1 < len(text):
            n = text[i + 1]
            out.append({"n": "\n", "t": "\t", "\\": "\\", '"': '"', "'": "'", "0": "\0"}.get(n, n))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def collect():
    """(context or None, msgid, plural or None, places) in the order the
    code has them."""
    found = {}
    for path in sorted((ROOT / "src").rglob("*.rs")):
        # Comments (doc examples) are not the code's strings.
        source = "\n".join("" if line.lstrip().startswith("//") else line for line in path.read_text(encoding="utf-8").splitlines())
        for pattern, kind in PATTERNS:
            for match in pattern.finditer(source):
                line = source.count("\n", 0, match.start()) + 1
                place = f"{path.relative_to(ROOT)}:{line}"
                if kind == "context":
                    key = (unescape(match.group(1)), unescape(match.group(2)), None)
                elif kind == "plural":
                    key = (None, unescape(match.group(1)), unescape(match.group(2)))
                else:
                    key = (None, unescape(match.group(1)), None)
                found.setdefault(key, []).append((path.relative_to(ROOT).as_posix(), line, place))
    ordered = sorted(found.items(), key=lambda item: (item[1][0][0], item[1][0][1]))
    return [(context, one, many, [p for _, _, p in places]) for (context, one, many), places in ordered]


def po_escape(text):
    return text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t")


def po_unescape(text):
    return unescape(text)


def parse(path):
    """(context, msgid, plural) -> msgstr or list of plural forms."""
    entries = {}
    if not path.exists():
        return entries
    current, field, index = {}, None, None
    def flush():
        if "msgid" in current and current["msgid"] != "":
            key = (current.get("msgctxt"), current["msgid"], current.get("msgid_plural"))
            entries[key] = current.get("plural") if "msgid_plural" in current else current.get("msgstr", "")
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            if not line:
                flush()
                current, field = {}, None
            continue
        m = re.match(r'(msgctxt|msgid|msgid_plural|msgstr)(?:\[(\d+)\])?\s+"(.*)"$', line)
        if m:
            field, index, value = m.group(1), m.group(2), po_unescape(m.group(3))
            if field == "msgstr" and index is not None:
                current.setdefault("plural", [])
                plural = current["plural"]
                while len(plural) <= int(index):
                    plural.append("")
                plural[int(index)] = value
                field = ("plural", int(index))
            else:
                current[field] = value
        elif line.startswith('"') and field is not None:
            value = po_unescape(line[1:-1])
            if isinstance(field, tuple):
                current["plural"][field[1]] += value
            else:
                current[field] += value
    flush()
    return entries


def forms(tag):
    return int(re.search(r"nplurals=(\d+)", HEADERS[tag][1]).group(1))


def write(tag, messages, translations):
    name, plural = HEADERS[tag]
    lines = [
        f"# CordFast's interface in {name}.",
        "# Kept in step with the code by scripts/i18n.py; empty messages show in English.",
        'msgid ""',
        'msgstr ""',
        '"Project-Id-Version: CordFast\\n"',
        f'"Language: {tag}\\n"',
        '"MIME-Version: 1.0\\n"',
        '"Content-Type: text/plain; charset=UTF-8\\n"',
        '"Content-Transfer-Encoding: 8bit\\n"',
        f'"Plural-Forms: {plural}\\n"',
        "",
    ]
    missing = 0
    for context, one, many, places in messages:
        lines.append("#: " + " ".join(places[:4]))
        if context is not None:
            lines.append(f'msgctxt "{po_escape(context)}"')
        lines.append(f'msgid "{po_escape(one)}"')
        value = translations.get((context, one, many))
        if many is None:
            text = value if isinstance(value, str) else ""
            missing += not text
            lines.append(f'msgstr "{po_escape(text)}"')
        else:
            lines.append(f'msgid_plural "{po_escape(many)}"')
            values = value if isinstance(value, list) else []
            values = (values + [""] * forms(tag))[: forms(tag)]
            missing += not all(values)
            for i, text in enumerate(values):
                lines.append(f'msgstr[{i}] "{po_escape(text)}"')
        lines.append("")
    (CATALOGS / f"{tag}.po").write_text("\n".join(lines), encoding="utf-8")
    return missing


def main(args):
    messages = collect()
    CATALOGS.mkdir(parents=True, exist_ok=True)
    if args[:1] == ["--import"]:
        tag, source = args[1], pathlib.Path(args[2])
        imported = json.loads(source.read_text(encoding="utf-8"))
        translations = parse(CATALOGS / f"{tag}.po")
        for context, one, many, _ in messages:
            key = one if context is None else f"{context}\u0004{one}"
            if key in imported:
                translations[(context, one, many)] = imported[key]
        missing = write(tag, messages, translations)
        print(f"{tag}: {len(messages)} messages, {missing} untranslated")
        return 0
    status = 0
    for tag in HEADERS:
        translations = parse(CATALOGS / f"{tag}.po")
        known = set(translations)
        missing_ids = [m for m in messages if (m[0], m[1], m[2]) not in known]
        if args[:1] == ["--check"]:
            if missing_ids:
                print(f"{tag}: {len(missing_ids)} messages missing, e.g. {missing_ids[0][1]!r}")
                status = 1
            continue
        missing = write(tag, messages, translations)
        print(f"{tag}: {len(messages)} messages, {missing} untranslated")
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
