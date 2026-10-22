[File Usage Guidelines]
You must choose the appropriate filename based on the current page URL route structure:

1. For List / Feed Pages (e.g., `cmsn.info/` or main feed):
   - Selector File: `selectors_list.json`
   - Data File: `items_list.json`

2. For Item Detail Pages (e.g., `cmsn.info/i/[id]` or any dynamic item route):
   - Selector File: `selectors_detail.json`
   - Data File: `items_detail.json`

[File Usage Rules]

- Write and Read data, selectors, in files. Reuse them for same route. Before trying to extract data, check if the data file already exists.
- When extracting selectors on a detail page (`/i/...`), save or read from `selectors_detail.json` so that all dynamic item routes reuse the exact same schema.
- When writing extracted item data, append to `items_detail.json` (or `items_list.json` depending on the page type).
- Do NOT generate unique filenames containing specific IDs (e.g., `items_AAAAA.json` is FORBIDDEN).
