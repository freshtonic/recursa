# recursa-explorer

`recursa-explorer` is the supported, publishable local browser UI for a
generated grammar. A development binary can start it with:

```rust,no_run
fn main() -> Result<(), recursa_explorer::RunError> {
    recursa_explorer::run(grammar::Value::explorer())
}
```

The launcher binds only to `127.0.0.1` on an operating-system-assigned port,
becomes ready before opening the system browser, and exits after **Stop
Explorer** requests orderly shutdown. HTML, CSS, and vanilla JavaScript are
compiled into the crate. There is no CDN, Node toolchain, framework, WASM,
project persistence, AST editor, or remote-bind option.

## Visual walkthrough

The three-column workspace keeps each interaction visible in context:

1. **Source** is an editable input with token colors and byte-accurate lexical
   or parse underlines, including for Unicode input. Editing immediately clears
   any stale tree; a strict failure displays structured diagnostics and no
   partial AST.
2. **AST** lazily expands Nodes and elements with variants, roles,
   cardinalities, exact token text, and spans. Point at source to select the
   deepest containing occurrence, or point at/click a tree row to highlight its
   source. Arrow keys, Home, End, Enter, and Space operate the ARIA tree.
3. **Grammar** lists static Node, variant, element, and token identities. When
   configured, **Pretty** renders source-map-linked output that can replace the
   editor, and **Sample** accepts a visible hexadecimal seed. Reusing a seed
   produces the same synthetic tree. Unsupported cards are hidden rather than
   pretending the capability exists.

## Manual browser smoke

The following launchers are repository-checkout-only fixtures and are excluded
from the published crate's explicit file allowlist. Published-crate consumers
use the general launcher call shown above.

Run the JSON smoke launcher (it opens the default browser):

```console
cargo run -p recursa-explorer --example json
```

Enter `{"items":[true,null,{"name":"café"}]}` and then verify:

- record the browser name/version and confirm developer tools show every ES
  module loaded under the response CSP with no console or network errors;
- nested and repeated valid input produces an expandable AST;
- hovering source and tree rows highlights in both directions, including text
  after `café`, an added emoji, and exactly between adjacent token spans;
- normal source clicking, caret placement, drag selection, scrolling, and
  keyboard editing remain available beneath the highlight overlay;
- invalid input immediately removes the previous AST and shows diagnostics;
- edit again while parsing is pending and confirm no stale tree or diagnostic
  replaces the newer result;
- Arrow keys, Home, End, Enter, and Space navigate/select the tree, with focus
  and expanded/selected state announced by accessibility tooling;
- a narrow viewport, 200% zoom, and forced-colors mode remain usable; and
- the workflow works with the network disconnected and ends cleanly through
  **Stop Explorer**.

Then run the one-Node capability fixture:

```console
cargo run -p recursa-explorer --example capabilities
```

Enter `ready`, verify Pretty links its output range back to source and **Apply
to source** reparses the result. Hover and keyboard-focus mapped Pretty ranges,
then edit source while rendering is pending and confirm stale output cannot
replace it. Enter the same valid hex seed twice and verify the status reports an
identical synthetic tree; change the seed while sampling is pending and confirm
the older result is discarded. Finish with **Stop Explorer**.

The browser uses an intentionally private, matching-version same-origin JSON
protocol to read schema, lex, parse, Pretty, and deterministic sample
projections. Strict parse failure returns diagnostics and no partial tree. The
server rejects nonlocal clients, unexpected Host or Origin headers, oversized
bodies, Pretty widths outside `1..=1000`, indentation steps outside `0..=32`,
unsupported methods and media types, and sends a restrictive content security
policy. It is not a public remote service or stable transport protocol. The
crate is publishable because the launcher and bundled UI are supported;
publication does not make its private loopback protocol a compatibility
contract.

Recursa uses Rust edition 2024 and requires Rust 1.88 or newer. See the
[workspace package map](https://github.com/freshtonic/recursa#workspace-packages).
