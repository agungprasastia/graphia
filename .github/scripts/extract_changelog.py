#!/usr/bin/env python3
"""
Extracts the release notes section for a given version/tag from CHANGELOG.md.
Follows Keep a Changelog formatting conventions.
"""
import sys
import re
from pathlib import Path

def extract_changelog(version: str, changelog_path: str = "CHANGELOG.md") -> str:
    path = Path(changelog_path)
    if not path.exists():
        return f"Release {version}"
    
    clean_ver = version.lstrip("v")
    text = path.read_text(encoding="utf-8")
    
    pattern = rf"##\s*\[?{re.escape(clean_ver)}\]?[^\n]*\n(.*?)(?=\n##\s*\[|\Z)"
    match = re.search(pattern, text, re.DOTALL)
    if match and match.group(1).strip():
        return match.group(1).strip()
    
    return f"Release {version}"

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: extract_changelog.py <version/tag> [changelog_path] [output_path]")
        sys.exit(1)
    
    ver = sys.argv[1]
    ch_path = sys.argv[2] if len(sys.argv) > 2 else "CHANGELOG.md"
    out_path = sys.argv[3] if len(sys.argv) > 3 else None
    
    notes = extract_changelog(ver, ch_path)
    if out_path:
        Path(out_path).write_text(notes + "\n", encoding="utf-8")
    else:
        print(notes)
