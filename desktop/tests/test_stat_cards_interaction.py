from pathlib import Path
import unittest


HTML = Path(__file__).resolve().parents[1] / "dist" / "index.html"


class StatCardsInteractionTest(unittest.TestCase):
    def test_summary_stats_are_clickable_filter_entrypoints(self):
        html = HTML.read_text(encoding="utf-8")

        expected_filters = ["all", "claude", "codex", "used", "unused", "broken"]
        for filter_name in expected_filters:
            self.assertIn(f'data-f="{filter_name}"', html)

        self.assertNotIn('class="stat"><div', html)
        self.assertIn('class="stat-btn', html)
        self.assertIn('id="toolsHead"', html)
        self.assertIn('function selectFilter(f)', html)
        self.assertIn('activeF==="used"&&cc>0', html)
        self.assertIn('selectFilter(f);', html)
        self.assertNotIn('scrollIntoView', html)
        self.assertNotIn('selectFilter(f, true)', html)


if __name__ == "__main__":
    unittest.main()
