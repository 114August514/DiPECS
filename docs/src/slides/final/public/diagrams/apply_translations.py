"""Apply translation dictionary to all SVG diagram files."""
import json
import re
import sys
from pathlib import Path

# Fix Windows console encoding for Unicode output
sys.stdout.reconfigure(encoding="utf-8")

DIAGRAMS_DIR = Path(r"e:\DIPECS\docs\src\slides\final\public\diagrams")
DICT_FILE = DIAGRAMS_DIR / "translations.json"

def load_translations():
    with open(DICT_FILE, "r", encoding="utf-8") as f:
        data = json.load(f)

    # Flatten all categories: English → Chinese
    # Skip crate_names (stay as-is) and _description
    all_translations = {}
    for category, entries in data.items():
        if category in ("_description", "crate_names"):
            continue
        for en, zh in entries.items():
            all_translations[en] = zh

    # Sort by length (longest first) to avoid partial matches
    sorted_items = sorted(all_translations.items(), key=lambda x: len(x[0]), reverse=True)
    return sorted_items


def translate_svg(filepath, translations):
    with open(filepath, "r", encoding="utf-8") as f:
        content = f.read()

    original = content

    def translate_text_element(match):
        before = match.group(1)
        inner = match.group(2)
        after = match.group(3)

        for en, zh in translations:
            if en in inner:
                inner = inner.replace(en, zh)

        return before + inner + after

    # Match <text ...>content</text>
    pattern = re.compile(r'(<text\b[^>]*>)(.*?)(</text>)', re.DOTALL)
    content = pattern.sub(translate_text_element, content)

    if content != original:
        with open(filepath, "w", encoding="utf-8") as f:
            f.write(content)
        return True
    return False


def main():
    translations = load_translations()
    print(f"Loaded {len(translations)} translation entries\n")

    svg_files = sorted(DIAGRAMS_DIR.glob("*.svg"))
    changed_count = 0
    for svg_file in svg_files:
        changed = translate_svg(svg_file, translations)
        if changed:
            changed_count += 1
            print(f"  [UPDATED] {svg_file.name}")
        else:
            print(f"  [no change] {svg_file.name}")

    print(f"\nDone: {changed_count} files updated, {len(svg_files) - changed_count} unchanged.")


if __name__ == "__main__":
    main()
