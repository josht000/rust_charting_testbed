use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub bid: f64,
    pub ask: f64,
    pub last: f64,
    pub timestamp_ms: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountSummary {
    pub account_id: String,
    pub status: String,
    pub equity: f64,
    pub cash: f64,
    pub buying_power: f64,
    pub day_pl: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
    Market,
    Limit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: Side,
    pub kind: OrderKind,
    pub quantity: f64,
    pub limit_price: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderRecord {
    pub id: String,
    pub request: OrderRequest,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderValidationError {
    EmptySymbol,
    InvalidSymbol,
    InvalidQuantity,
    MissingLimitPrice,
    InvalidLimitPrice,
    InvalidNotional,
}

impl std::fmt::Display for OrderValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptySymbol => "Enter a symbol.",
            Self::InvalidSymbol => "Use letters, digits, dot, dash, or slash in the symbol.",
            Self::InvalidQuantity => "Quantity must be finite and greater than zero.",
            Self::MissingLimitPrice => "A limit order needs a limit price.",
            Self::InvalidLimitPrice => "Limit price must be finite and greater than zero.",
            Self::InvalidNotional => "Limit order notional must be finite and greater than zero.",
        };
        f.write_str(message)
    }
}

impl std::error::Error for OrderValidationError {}

pub fn normalize_symbol(symbol: &str) -> String {
    symbol.trim().to_ascii_uppercase()
}

pub fn validate_order(request: &OrderRequest) -> Result<(), OrderValidationError> {
    let symbol = normalize_symbol(&request.symbol);
    if symbol.is_empty() {
        return Err(OrderValidationError::EmptySymbol);
    }
    if !symbol
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'/'))
        || !symbol.bytes().any(|b| b.is_ascii_alphanumeric())
    {
        return Err(OrderValidationError::InvalidSymbol);
    }
    if !request.quantity.is_finite() || request.quantity <= 0.0 {
        return Err(OrderValidationError::InvalidQuantity);
    }
    if request.kind == OrderKind::Limit {
        let Some(price) = request.limit_price else {
            return Err(OrderValidationError::MissingLimitPrice);
        };
        if !price.is_finite() || price <= 0.0 {
            return Err(OrderValidationError::InvalidLimitPrice);
        }
        let notional = request.quantity * price;
        if !notional.is_finite() || notional <= 0.0 {
            return Err(OrderValidationError::InvalidNotional);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_request() -> OrderRequest {
        OrderRequest {
            symbol: " spy ".into(),
            side: Side::Buy,
            kind: OrderKind::Limit,
            quantity: 2.0,
            limit_price: Some(540.25),
        }
    }

    #[test]
    fn normalizes_symbol_without_changing_internal_separator() {
        assert_eq!(normalize_symbol(" btc/usd "), "BTC/USD");
    }

    #[test]
    fn accepts_valid_limit_order() {
        assert_eq!(validate_order(&valid_request()), Ok(()));
    }

    #[test]
    fn rejects_non_finite_or_non_positive_quantities() {
        for quantity in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut request = valid_request();
            request.quantity = quantity;
            assert_eq!(
                validate_order(&request),
                Err(OrderValidationError::InvalidQuantity)
            );
        }
    }

    #[test]
    fn rejects_missing_or_invalid_limit_price() {
        let mut request = valid_request();
        request.limit_price = None;
        assert_eq!(
            validate_order(&request),
            Err(OrderValidationError::MissingLimitPrice)
        );
        for price in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            request.limit_price = Some(price);
            assert_eq!(
                validate_order(&request),
                Err(OrderValidationError::InvalidLimitPrice)
            );
        }
    }

    #[test]
    fn rejects_unsafe_and_punctuation_only_symbols() {
        for symbol in ["SPY;BUY", ".", "-", "/", "//"] {
            let mut request = valid_request();
            request.symbol = symbol.into();
            assert_eq!(
                validate_order(&request),
                Err(OrderValidationError::InvalidSymbol)
            );
        }
    }

    #[test]
    fn rejects_zero_or_overflowing_limit_notionals() {
        for (quantity, price) in [(f64::MIN_POSITIVE, f64::MIN_POSITIVE), (f64::MAX, f64::MAX)] {
            let mut request = valid_request();
            request.quantity = quantity;
            request.limit_price = Some(price);
            assert_eq!(
                validate_order(&request),
                Err(OrderValidationError::InvalidNotional)
            );
        }
    }
}
