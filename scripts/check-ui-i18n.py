#!/usr/bin/env python3
"""Check production Rust UI literals, excluding comments and test-only items."""
from pathlib import Path
import re
import sys

TOKEN = re.compile(
    r'(?P<test>\#\s*\[\s*(?:cfg\s*\(\s*test\s*\)|test)\s*\])'
    r'|(?P<line>//[^\n]*)|(?P<block>/\*)'
    r'|(?P<raw>\b(?:br|r)(?P<hashes>\#*)".*?"(?P=hashes))'
    r'|(?P<string>b?"(?:\\.|[^"\\])*")'
    r"|(?P<char>b?'(?:\\.|[^'\\])')"
    r'|(?P<punct>[{};])',
    re.DOTALL,
)
CJK = re.compile(r'[\u4e00-\u9fff]')


def violations(source):
    position, test_depth, pending_test = 0, 0, False
    while match := TOKEN.search(source, position):
        position = match.end()
        kind, value = match.lastgroup, match.group()
        if kind == 'block':
            depth = 1
            while depth:
                delimiter = re.search(r'/\*|\*/', source[position:])
                if delimiter is None:
                    position = len(source)
                    break
                depth += 1 if delimiter.group() == '/*' else -1
                position += delimiter.end()
            continue
        if test_depth:
            if kind == 'punct':
                test_depth += (value == '{') - (value == '}')
            continue
        if kind == 'test':
            pending_test = True
        elif pending_test:
            if kind == 'punct' and value in '{;':
                test_depth = int(value == '{')
                pending_test = False
        elif kind in ('raw', 'string') and CJK.search(value):
            yield source.count('\n', 0, match.start()) + 1


def main():
    root = Path(__file__).resolve().parents[1] / 'src-ui/src'
    failures = []
    for path in sorted(root.rglob('*.rs')):
        if path.name == 'i18n.rs' or 'tests' in path.relative_to(root).parts:
            continue
        failures.extend(f'{path.relative_to(root)}:{line}'
                        for line in violations(path.read_text()))
    if failures:
        print('Move production CJK literals into i18n.rs:\n' + '\n'.join(failures))
        return 1
    print('UI production literals pass the i18n check')
    return 0


if __name__ == '__main__':
    sys.exit(main())
