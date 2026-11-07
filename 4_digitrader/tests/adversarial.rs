use digitrader::model::{
    normalize_symbol, validate_order, AccountSummary, OrderKind, OrderRecord, OrderRequest,
    OrderValidationError, Quote, Side,
};

const ACCOUNT_FIXTURE: &str = include_str!("../fixtures/account.json");
const QUOTES_FIXTURE: &str = include_str!("../fixtures/quotes.json");
const UI_SOURCE: &str = include_str!("../ui/app.slint");

fn request(symbol: &str, quantity: f64, kind: OrderKind, limit_price: Option<f64>) -> OrderRequest {
    OrderRequest {
        symbol: symbol.to_owned(),
        side: Side::Buy,
        kind,
        quantity,
        limit_price,
    }
}

#[test]
fn rejects_non_finite_and_non_positive_quantities() {
    for quantity in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -0.0,
        -f64::MIN_POSITIVE,
    ] {
        assert_eq!(
            validate_order(&request("SPY", quantity, OrderKind::Market, None)),
            Err(OrderValidationError::InvalidQuantity),
            "unexpected result for quantity {quantity:?}"
        );
    }
}

#[test]
fn rejects_non_finite_and_non_positive_limit_prices() {
    for price in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -0.0,
        -f64::MIN_POSITIVE,
    ] {
        assert_eq!(
            validate_order(&request("SPY", 1.0, OrderKind::Limit, Some(price))),
            Err(OrderValidationError::InvalidLimitPrice),
            "unexpected result for limit price {price:?}"
        );
    }
}

#[test]
fn rejects_symbols_with_non_ascii_or_injection_characters() {
    for symbol in ["SPY;BUY", "BRK B", "SP\nY", "ÄBC", "SPY\0"] {
        assert_eq!(
            validate_order(&request(symbol, 1.0, OrderKind::Market, None)),
            Err(OrderValidationError::InvalidSymbol),
            "unexpected result for symbol {symbol:?}"
        );
    }
}

#[test]
fn rejects_empty_symbols_after_trimming() {
    for symbol in ["", " ", "\t\n"] {
        assert_eq!(
            validate_order(&request(symbol, 1.0, OrderKind::Market, None)),
            Err(OrderValidationError::EmptySymbol)
        );
    }
}

#[test]
fn rejects_punctuation_only_symbols() {
    for symbol in [".", "-", "/", "//"] {
        assert_eq!(
            validate_order(&request(symbol, 1.0, OrderKind::Market, None)),
            Err(OrderValidationError::InvalidSymbol),
            "punctuation-only symbol {symbol:?} must be rejected"
        );
    }
}

#[test]
fn symbol_normalization_trims_and_uppercases_ascii() {
    assert_eq!(normalize_symbol("  btc/usd  "), "BTC/USD");
    assert_eq!(normalize_symbol("aapl"), "AAPL");
}

#[test]
fn rejects_underflowing_or_overflowing_limit_notionals() {
    for (quantity, price) in [(f64::MIN_POSITIVE, f64::MIN_POSITIVE), (f64::MAX, f64::MAX)] {
        assert_eq!(
            validate_order(&request("SPY", quantity, OrderKind::Limit, Some(price))),
            Err(OrderValidationError::InvalidNotional)
        );
    }
}

#[test]
fn market_order_does_not_require_a_limit_price() {
    assert_eq!(
        validate_order(&request("SPY", 1.0, OrderKind::Market, None)),
        Ok(())
    );
}

#[test]
fn order_request_and_record_survive_json_round_trips() {
    let order = OrderRecord {
        id: "sim-42".into(),
        request: request(" btc/usd ", 0.125, OrderKind::Limit, Some(101_000.25)),
        status: "preview".into(),
    };

    let encoded = serde_json::to_string(&order).expect("valid order should serialize");
    let decoded: OrderRecord =
        serde_json::from_str(&encoded).expect("serialized order should deserialize");
    assert_eq!(decoded, order);
    assert!(encoded.contains("\"limit\""));
    assert!(encoded.contains("\"buy\""));
}

#[test]
fn non_finite_order_values_do_not_round_trip_as_valid_numbers() {
    let order = request("SPY", f64::NAN, OrderKind::Market, None);
    let encoded = serde_json::to_string(&order).expect("serde_json serializes NaN as null");
    assert!(
        serde_json::from_str::<OrderRequest>(&encoded).is_err(),
        "non-finite values must not silently become a valid numeric order"
    );
}

#[test]
fn static_account_fixture_is_valid_and_internally_consistent() {
    let account: AccountSummary =
        serde_json::from_str(ACCOUNT_FIXTURE).expect("account fixture must be valid JSON");
    assert!(!account.account_id.trim().is_empty());
    assert!(!account.status.trim().is_empty());
    for value in [
        account.equity,
        account.cash,
        account.buying_power,
        account.day_pl,
    ] {
        assert!(value.is_finite());
    }
    assert!(account.equity > 0.0);
    assert!(account.cash >= 0.0 && account.cash <= account.equity);
    assert!(account.buying_power >= 0.0);
}

#[test]
fn static_quote_fixture_has_unique_valid_symbols_and_ordered_prices() {
    let quotes: Vec<Quote> =
        serde_json::from_str(QUOTES_FIXTURE).expect("quote fixture must be valid JSON");
    assert!(!quotes.is_empty());

    let mut symbols = std::collections::HashSet::new();
    for quote in quotes {
        let normalized = normalize_symbol(&quote.symbol);
        assert!(!normalized.is_empty());
        assert!(
            symbols.insert(normalized.clone()),
            "duplicate quote symbol after normalization: {normalized}"
        );
        assert_eq!(
            validate_order(&request(&quote.symbol, 1.0, OrderKind::Market, None)),
            Ok(()),
            "invalid quote symbol: {}",
            quote.symbol
        );
        assert!(quote.timestamp_ms > 0);
        assert!([quote.bid, quote.ask, quote.last]
            .iter()
            .all(|price| price.is_finite() && *price > 0.0));
        assert!(quote.bid <= quote.last && quote.last <= quote.ask);
    }
}

#[test]
fn ui_has_a_reasonable_minimum_window_size_and_live_symbol_chart_hooks() {
    assert!(UI_SOURCE.contains("min-width: 800px;"));
    assert!(UI_SOURCE.contains("min-height: 520px;"));
    assert!(UI_SOURCE.contains("preferred-width: 1540px;"));
    assert!(UI_SOURCE.contains("preferred-height: 920px;"));
    assert!(UI_SOURCE.contains("root.chart-symbol-submitted(chart.id, self.text)"));
    assert!(UI_SOURCE.contains("root.chart-resized(chart.id, self.width / 1px, self.height / 1px)"));
    assert!(UI_SOURCE.contains("root.chart-pointer(self.mouse-x / 1px, self.mouse-y / 1px, true)"));
}

#[test]
fn order_review_button_is_connected_to_the_declared_order_callback() {
    assert!(UI_SOURCE.contains("callback order-submitted("));
    assert!(
        UI_SOURCE.contains("root.order-submitted("),
        "order-submitted is declared but no UI control currently invokes it"
    );
}
