# TickerTapeTallyBoard
A portfolio management application

## Usage

Run the Sharesight import spike against the local private export:

```powershell
cd backend
cargo run --example sharesight_import_spike
```

To verify the split-position invariant when the current Sharesight `NOW` position is known:

```powershell
cd backend
cargo run --example sharesight_import_spike -- --split-current-position <CURRENT_NOW_POSITION>
```
