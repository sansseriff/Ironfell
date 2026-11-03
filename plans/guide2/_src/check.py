#!/usr/bin/env python3
"""Check generated guide structure and local links (no third-party packages)."""
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent


class Page(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.ids = Counter()
        self.links = []
        self.headings = 0
        self.unexpanded = []
        self.filters = set()
        self.systems = set()
        self.feed(path.read_text())

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids[attrs['id']] += 1
        if tag == 'h1':
            self.headings += 1
        if tag == 'button' and 'data-sys' in attrs:
            self.filters.add(attrs['data-sys'])
        if tag == 'tr' and 'data-sys' in attrs:
            self.systems.add(attrs['data-sys'])
        if tag in ('snip', 'ref', 'raw', 'out', 'chip'):
            self.unexpanded.append(tag)
        for attr in ('href', 'src'):
            if attr in attrs:
                self.links.append(attrs[attr])


def main():
    pages = {p.resolve(): Page(p) for p in ROOT.glob('*.html')}
    errors = []
    links = 0
    for path, page in pages.items():
        if page.headings != 1:
            errors.append(f'{path.name}: expected one h1, got {page.headings}')
        for name, count in page.ids.items():
            if count > 1:
                errors.append(f'{path.name}: duplicate id {name}')
        if page.unexpanded:
            errors.append(f'{path.name}: unexpanded tags {page.unexpanded}')
        if page.systems - page.filters:
            errors.append(f'{path.name}: missing system filters {sorted(page.systems - page.filters)}')
        for href in page.links:
            url = urlsplit(href)
            if url.scheme or url.netloc:
                continue
            links += 1
            target = (path.parent / unquote(url.path)).resolve() if url.path else path
            if not target.exists():
                errors.append(f'{path.name}: missing {href}')
            elif url.fragment and target in pages and unquote(url.fragment) not in pages[target].ids:
                errors.append(f'{path.name}: missing fragment {href}')
    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'ok: {len(pages)} pages, {links} local links, unique ids, one h1 per page, no source tags')


if __name__ == '__main__':
    main()
