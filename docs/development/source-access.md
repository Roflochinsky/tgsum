# Source access in the desktop UI

The source card describes the operation implemented in this desktop build.
`project_source_accesses` reads Project metadata and derives each method from
the same importer binding used by `refresh_project_source`. It does not open
archives, snapshots, messenger clients or credential stores. Its revision check
rejects stale Project metadata. Coverage comes from the existing snapshot preview,
so showing access facts does not load a large snapshot a second time.

The current binding is `telegram_json` with platform `telegram`: local JSON,
no import credentials, archive reimport, attachment references. A saved unknown
connector, including a future OAuth source, has no verified method. The UI
disables its archive refresh/relink controls and does not infer a token grant
from its selected conversation. Adding another binding requires corresponding
access facts; the Telegram explanation is not a generic OAuth explanation.

## Facts visible to the user

- Before import: the chosen JSON is read locally and connected chats are saved.
- Source details: the whole input file is read; other chats in a full export
  can be read during indexing/validation. The stored snapshot contains the
  original selected conversation. Topic/date/delta selection and sanitization
  narrow the prepared analysis context, not that raw stored snapshot.
- The local account label is a namespace, not proof of account ownership.
- Import needs no messenger login. The selected client handles login/export;
  TGSUM does not read its session store. The selected-folder watcher notices
  candidate JSON files while the app is running and lists them in the source
  card, but a human confirms
  completed export and scope before import. Automatic client export and account
  safety guarantees are not advertised.
- Attachment choices are saved selections, not a count of included files.
  Preparation checks them; Review shows the actual included count.
- Coverage describes the current snapshot. Missing or unreadable snapshots
  cannot establish completeness. Telegram exports currently report unknown.
- Import/preparation are local. Analyze and Review show the chosen destination
  before explicit Run; Export only writes a local context bundle.

Dynamic identities are inserted as text. The Rust command regression checks a
nonexistent archive, an unknown OAuth connector, revision conflicts and unchanged
Project state. The actual WebView suite in `scripts/desktop-e2e.py` exercises
access details, a markup-like account label, unknown-connector controls and
destination labels using synthetic files/agents. This does not qualify a new
messenger API, native export automation or real account authorization.
