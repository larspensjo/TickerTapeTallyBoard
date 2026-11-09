//! Conviction close-position guard integration tests.
use super::*;

// --- Conviction close-position guard -------------------------------------

// A Sharesight report that sells the entire MSFT position, closing it.
const CLOSE_MSFT: &str = concat!(
    "Synthetic - All Trades Report between 2025-06-12 and 2026-06-12\n",
    "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
    "NASDAQ,MSFT,Microsoft,Sell,13/06/2026,\u{2212}10,\"12,60\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"\u{2212}1260,00\",All Trades,\n",
);

/// Create MSFT with an open 10-share position and the given conviction.
async fn setup_msft_position(state: &AppState, conviction: &str) -> i64 {
    let (status, body) = send_json_body(
        state,
        "POST",
        "/api/instruments",
        json!({"symbol":"MSFT","exchange":"NASDAQ","name":"Microsoft","type":"Stock","currency":"USD"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_i64().expect("instrument id");

    let (status, _) = send_json_body(
        state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"12.50","currency":"USD","fx_rate_to_base":"0.1"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = send_json_body(
        state,
        "PUT",
        &format!("/api/instruments/{id}/conviction"),
        json!({ "conviction": conviction }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    id
}

async fn conviction_of(state: &AppState, id: i64) -> String {
    let (_, list) = send_json(state, "GET", "/api/instruments").await;
    list.as_array()
        .expect("array")
        .iter()
        .find(|row| row["id"].as_i64() == Some(id))
        .expect("instrument present")["conviction"]
        .as_str()
        .expect("conviction")
        .to_owned()
}

#[tokio::test]
async fn preview_blocks_when_import_closes_convicted_position() {
    let state = test_state().await;
    setup_msft_position(&state, "High").await;

    let (status, body) = send_bytes(
        &state,
        "/api/import/sharesight/preview",
        CLOSE_MSFT.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let closing = body["conviction_close_positions"]
        .as_array()
        .expect("array");
    assert_eq!(closing.len(), 1);
    assert_eq!(closing[0]["asset_key"], "nasdaq:msft");
    assert_eq!(closing[0]["symbol"], "MSFT");
    assert_eq!(closing[0]["conviction"], "High");
}

#[tokio::test]
async fn preview_does_not_block_when_conviction_is_other() {
    let state = test_state().await;
    setup_msft_position(&state, "Other").await;

    let (status, body) = send_bytes(
        &state,
        "/api/import/sharesight/preview",
        CLOSE_MSFT.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["conviction_close_positions"]
        .as_array()
        .expect("array")
        .is_empty());
}

#[tokio::test]
async fn commit_without_conviction_choice_is_rejected() {
    let state = test_state().await;
    setup_msft_position(&state, "High").await;

    let (status, body) = send_bytes(
        &state,
        "/api/import/sharesight/commit",
        CLOSE_MSFT.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "conviction_choice_required");
}

#[tokio::test]
async fn commit_keeping_conviction_preserves_it() {
    let state = test_state().await;
    let id = setup_msft_position(&state, "High").await;

    let (status, _) = send_bytes(
        &state,
        "/api/import/sharesight/commit?conviction_keep=nasdaq:msft",
        CLOSE_MSFT.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(conviction_of(&state, id).await, "High");
}

#[tokio::test]
async fn commit_changing_conviction_to_other_clears_it() {
    let state = test_state().await;
    let id = setup_msft_position(&state, "High").await;

    let (status, _) = send_bytes(
        &state,
        "/api/import/sharesight/commit?conviction_to_other=nasdaq:msft",
        CLOSE_MSFT.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(conviction_of(&state, id).await, "Other");
}

#[tokio::test]
async fn rollback_leaves_conviction_unchanged() {
    let state = test_state().await;
    let id = setup_msft_position(&state, "High").await;

    let (status, committed) = send_bytes(
        &state,
        "/api/import/sharesight/commit?conviction_keep=nasdaq:msft",
        CLOSE_MSFT.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let (status, _) = send_json(&state, "POST", &format!("/api/import/rollback/{batch_id}")).await;
    assert_eq!(status, StatusCode::OK);

    // Rollback restores the open position and never touches conviction.
    assert_eq!(conviction_of(&state, id).await, "High");
    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings["holdings"].as_array().expect("array").len(), 1);
    assert_eq!(holdings["holdings"][0]["quantity"], 10);
}

// A report that closes MSFT (convicted) and buys a second, new asset.
const CLOSE_MSFT_PLUS_NEW: &str = concat!(
    "Synthetic - All Trades Report between 2025-06-12 and 2026-06-12\n",
    "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
    "NASDAQ,MSFT,Microsoft,Sell,13/06/2026,\u{2212}10,\"12,60\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"\u{2212}1260,00\",All Trades,\n",
    "NASDAQ,AAPL,Apple,Buy,13/06/2026,5,\"150,00\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"750,00\",All Trades,\n",
);

#[tokio::test]
async fn deselected_closing_asset_does_not_require_a_conviction_choice() {
    let state = test_state().await;
    let id = setup_msft_position(&state, "High").await;

    // Exclude MSFT: its closing sell is dropped, so the guard must not block and
    // the commit writes only the other asset.
    let (status, _) = send_bytes(
        &state,
        "/api/import/sharesight/commit?exclude=nasdaq:msft",
        CLOSE_MSFT_PLUS_NEW.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // MSFT stays open with its conviction intact; only AAPL was imported.
    assert_eq!(conviction_of(&state, id).await, "High");
    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    let msft = holdings["holdings"]
        .as_array()
        .expect("array")
        .iter()
        .find(|h| h["instrument"]["symbol"] == "MSFT")
        .expect("MSFT still open");
    assert_eq!(msft["quantity"], 10);
}

// Minimal Avanza reports for a refresh (replace) guard scenario: batch 1 buys 5
// Apple; the refresh re-exports that buy (already-imported) plus a new closing
// sell, so Apple is a "mixed" instrument whose position the refresh closes.
const AV_APPLE_BUY: &str = concat!(
    "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
    "2026-05-10;ISK;Köp;Apple Inc;5;150,00;-7500,00;SEK;9,00;9,40;USD;US0378331005;\n",
);
const AV_APPLE_REFRESH_CLOSE: &str = concat!(
    "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
    "2026-06-20;ISK;Sälj;Apple Inc;-5;210,00;10500,00;SEK;9,00;9,45;USD;US0378331005;300,00\n",
    "2026-05-10;ISK;Köp;Apple Inc;5;150,00;-7500,00;SEK;9,00;9,40;USD;US0378331005;\n",
);

#[tokio::test]
async fn avanza_refresh_blocks_and_clears_conviction_on_closing_position() {
    let state = test_state().await;

    // Batch 1: buy 5 Apple, then mark it High conviction.
    let (status, _) =
        send_bytes(&state, "/api/import/avanza/commit", AV_APPLE_BUY.as_bytes()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, instruments) = send_json(&state, "GET", "/api/instruments").await;
    let id = instruments[0]["id"].as_i64().expect("instrument id");
    let (status, _) = send_json_body(
        &state,
        "PUT",
        &format!("/api/instruments/{id}/conviction"),
        json!({ "conviction": "High" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Refresh preview flags Apple as a closing convicted position.
    let (status, preview) = send_bytes(
        &state,
        "/api/import/avanza/preview",
        AV_APPLE_REFRESH_CLOSE.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let batch_id = preview["replace_candidate_batch_id"]
        .as_i64()
        .expect("replace candidate");
    let closing = preview["conviction_close_positions"]
        .as_array()
        .expect("array");
    assert_eq!(
        closing.len(),
        1,
        "refresh should flag the closing Apple position"
    );
    assert_eq!(closing[0]["asset_key"], "US0378331005");

    // Refresh commit without a choice is rejected.
    let (status, body) = send_bytes(
        &state,
        &format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}"),
        AV_APPLE_REFRESH_CLOSE.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "conviction_choice_required");

    // With change-to-other the refresh commits and clears the conviction.
    let (status, _) = send_bytes(
        &state,
        &format!(
            "/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}&conviction_to_other=US0378331005"
        ),
        AV_APPLE_REFRESH_CLOSE.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(conviction_of(&state, id).await, "Other");
}
