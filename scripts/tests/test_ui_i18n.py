import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('ui_i18n', Path(__file__).parents[1] / 'check-ui-i18n.py')
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class ProductionLiterals(unittest.TestCase):
    def test_comments_and_test_items_are_not_ui_literals(self):
        source = '''// "说明"
/* nested /* "说明" */ "说明" */
#[cfg(test)] use helper::example;
#[cfg(test)] mod tests { fn example() { assert_eq!(value, "中文 { }"); } }
#[test] fn example() { assert_eq!(value, "中文"); }
let label = "中文";
'''
        self.assertEqual(list(guard.violations(source)), [6])

    def test_regular_and_raw_strings_cannot_hide_behind_comment_markers(self):
        source = 'let a = "https://example/中文";\nlet b = r##"/* 中文 */"##;'
        self.assertEqual(list(guard.violations(source)), [1, 2])


if __name__ == '__main__':
    unittest.main()
