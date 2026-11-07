use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{self, AccountSummary, OrderKind, OrderRequest, Quote, Side};

pub trait BrokerProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, BrokerError>;
    fn account_summary(&self) -> Result<AccountSummary, BrokerError>;
    fn preview_order(&self, request: &OrderRequest) -> Result<OrderPreview, BrokerError>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderPreview {
    pub request: OrderRequest,
    pub reference_price: f64,
    pub estimated_notional: f64,
}

#[derive(Debug)]
pub enum BrokerError {
    InvalidLookupSymbol,
    FixtureMissing { fixture: String },
    FixtureRead { fixture: String, message: String },
    MalformedFixture { fixture: String, message: String },
    QuoteNotFound { symbol: String },
    InvalidFixtureData { fixture: String, message: String },
    InvalidOrder(model::OrderValidationError),
    PreviewValueOutOfRange,
}

impl fmt::Display for BrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLookupSymbol => f.write_str("The quote symbol is empty or invalid."),
            Self::FixtureMissing { fixture } => write!(f, "Fixture is missing: {fixture}"),
            Self::FixtureRead { fixture, message } => {
                write!(f, "Could not read fixture {fixture}: {message}")
            }
            Self::MalformedFixture { fixture, message } => {
                write!(f, "Fixture {fixture} is malformed: {message}")
            }
            Self::QuoteNotFound { symbol } => write!(f, "No quote fixture exists for {symbol}."),
            Self::InvalidFixtureData { fixture, message } => {
                write!(f, "Fixture {fixture} contains invalid data: {message}")
            }
            Self::InvalidOrder(error) => write!(f, "Invalid order: {error}"),
            Self::PreviewValueOutOfRange => {
                f.write_str("The estimated order notional is out of range.")
            }
        }
    }
}

impl std::error::Error for BrokerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidOrder(error) => Some(error),
            _ => None,
        }
    }
}

enum FixtureSource {
    Directory(PathBuf),
    Embedded,
}

pub struct FixtureBroker {
    source: FixtureSource,
}

impl FixtureBroker {
    pub fn new(fixture_dir: impl Into<PathBuf>) -> Self {
        Self {
            source: FixtureSource::Directory(fixture_dir.into()),
        }
    }

    pub fn from_embedded() -> Self {
        Self {
            source: FixtureSource::Embedded,
        }
    }

    pub fn bundled() -> Self {
        Self::from_embedded()
    }

    fn fixture_path(&self, name: &str) -> Option<PathBuf> {
        match &self.source {
            FixtureSource::Directory(directory) => Some(directory.join(name)),
            FixtureSource::Embedded => None,
        }
    }

    fn read_fixture(&self, name: &str) -> Result<Vec<u8>, BrokerError> {
        let Some(path) = self.fixture_path(name) else {
            let contents = match name {
                "quotes.json" => include_str!("../fixtures/quotes.json"),
                "account.json" => include_str!("../fixtures/account.json"),
                _ => {
                    return Err(BrokerError::FixtureMissing {
                        fixture: name.to_owned(),
                    })
                }
            };
            return Ok(contents.as_bytes().to_vec());
        };
        fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BrokerError::FixtureMissing {
                    fixture: display_path(&path),
                }
            } else {
                BrokerError::FixtureRead {
                    fixture: display_path(&path),
                    message: error.to_string(),
                }
            }
        })
    }

    fn load_quotes(&self) -> Result<Vec<Quote>, BrokerError> {
        let name = "quotes.json";
        let fixture = self.fixture_path(name);
        let bytes = self.read_fixture(name)?;
        let mut quotes: Vec<Quote> =
            serde_json::from_slice(&bytes).map_err(|error| BrokerError::MalformedFixture {
                fixture: fixture
                    .as_deref()
                    .map(display_path)
                    .unwrap_or_else(|| name.to_owned()),
                message: error.to_string(),
            })?;

        for quote in &mut quotes {
            quote.symbol = model::normalize_symbol(&quote.symbol);
            if !valid_symbol(&quote.symbol) {
                return Err(BrokerError::InvalidFixtureData {
                    fixture: fixture
                        .as_deref()
                        .map(display_path)
                        .unwrap_or_else(|| name.to_owned()),
                    message: format!("invalid quote symbol {:?}", quote.symbol),
                });
            }
            if !quote.bid.is_finite()
                || quote.bid <= 0.0
                || !quote.ask.is_finite()
                || quote.ask <= 0.0
                || !quote.last.is_finite()
                || quote.last <= 0.0
                || quote.bid > quote.ask
                || quote.last < quote.bid
                || quote.last > quote.ask
            {
                return Err(BrokerError::InvalidFixtureData {
                    fixture: fixture
                        .as_deref()
                        .map(display_path)
                        .unwrap_or_else(|| name.to_owned()),
                    message: format!("invalid prices for {}", quote.symbol),
                });
            }
        }
        for index in 0..quotes.len() {
            if quotes[..index]
                .iter()
                .any(|previous| previous.symbol == quotes[index].symbol)
            {
                return Err(BrokerError::InvalidFixtureData {
                    fixture: fixture
                        .as_deref()
                        .map(display_path)
                        .unwrap_or_else(|| name.to_owned()),
                    message: format!("duplicate quote symbol {}", quotes[index].symbol),
                });
            }
        }
        Ok(quotes)
    }

    fn load_account(&self) -> Result<AccountSummary, BrokerError> {
        let name = "account.json";
        let fixture = self.fixture_path(name);
        let bytes = self.read_fixture(name)?;
        let account: AccountSummary =
            serde_json::from_slice(&bytes).map_err(|error| BrokerError::MalformedFixture {
                fixture: fixture
                    .as_deref()
                    .map(display_path)
                    .unwrap_or_else(|| name.to_owned()),
                message: error.to_string(),
            })?;
        if account.account_id.trim().is_empty()
            || !account.equity.is_finite()
            || !account.cash.is_finite()
            || !account.buying_power.is_finite()
            || !account.day_pl.is_finite()
        {
            return Err(BrokerError::InvalidFixtureData {
                fixture: fixture
                    .as_deref()
                    .map(display_path)
                    .unwrap_or_else(|| name.to_owned()),
                message: "account fields must be populated with finite values".to_owned(),
            });
        }
        Ok(account)
    }
}

impl BrokerProvider for FixtureBroker {
    fn quote(&self, symbol: &str) -> Result<Quote, BrokerError> {
        let symbol = model::normalize_symbol(symbol);
        if !valid_symbol(&symbol) {
            return Err(BrokerError::InvalidLookupSymbol);
        }
        self.load_quotes()?
            .into_iter()
            .find(|quote| quote.symbol == symbol)
            .ok_or(BrokerError::QuoteNotFound { symbol })
    }

    fn account_summary(&self) -> Result<AccountSummary, BrokerError> {
        self.load_account()
    }

    fn preview_order(&self, request: &OrderRequest) -> Result<OrderPreview, BrokerError> {
        model::validate_order(request).map_err(BrokerError::InvalidOrder)?;
        let mut normalized_request = request.clone();
        normalized_request.symbol = model::normalize_symbol(&normalized_request.symbol);
        let quote = self.quote(&normalized_request.symbol)?;
        let reference_price = match &normalized_request.kind {
            OrderKind::Limit => normalized_request
                .limit_price
                .expect("validated limit price"),
            OrderKind::Market => match &normalized_request.side {
                Side::Buy => quote.ask,
                Side::Sell => quote.bid,
            },
        };
        let estimated_notional = reference_price * normalized_request.quantity;
        if !estimated_notional.is_finite() || estimated_notional <= 0.0 {
            return Err(BrokerError::PreviewValueOutOfRange);
        }
        Ok(OrderPreview {
            request: normalized_request,
            reference_price,
            estimated_notional,
        })
    }
}

fn valid_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol.bytes().any(|byte| byte.is_ascii_alphanumeric())
        && symbol
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'/'))
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("broker_fixtures")
            .join(name)
    }

    fn broker() -> FixtureBroker {
        FixtureBroker::new(fixtures("valid"))
    }

    #[test]
    fn looks_up_stock_and_crypto_quotes_with_normalized_symbols() {
        let broker = broker();
        assert_eq!(broker.quote(" spy ").unwrap().symbol, "SPY");
        assert_eq!(broker.quote("btc/usd").unwrap().symbol, "BTC/USD");
    }

    #[test]
    fn bundled_fixtures_support_quote_account_and_preview_workflows() {
        let broker = FixtureBroker::from_embedded();
        assert_eq!(broker.quote("btc/usd").unwrap().symbol, "BTC/USD");
        assert_eq!(
            FixtureBroker::bundled()
                .account_summary()
                .unwrap()
                .account_id,
            "SIM-001"
        );
        let preview = broker
            .preview_order(&OrderRequest {
                symbol: "SPY".into(),
                side: Side::Sell,
                kind: OrderKind::Market,
                quantity: 1.0,
                limit_price: None,
            })
            .unwrap();
        assert_eq!(preview.estimated_notional, 540.24);
    }

    #[test]
    fn reports_missing_quote_and_invalid_lookup_symbol() {
        assert!(matches!(
            broker().quote("MSFT"),
            Err(BrokerError::QuoteNotFound { symbol }) if symbol == "MSFT"
        ));
        assert!(matches!(
            broker().quote("SPY;BUY"),
            Err(BrokerError::InvalidLookupSymbol)
        ));
        assert!(matches!(
            broker().quote("---"),
            Err(BrokerError::InvalidLookupSymbol)
        ));
    }

    #[test]
    fn reports_missing_and_malformed_fixtures() {
        assert!(matches!(
            FixtureBroker::new(fixtures("missing")).quote("SPY"),
            Err(BrokerError::FixtureMissing { .. })
        ));
        assert!(matches!(
            FixtureBroker::new(fixtures("malformed")).quote("SPY"),
            Err(BrokerError::MalformedFixture { .. })
        ));
        assert!(matches!(
            FixtureBroker::new(fixtures("last-out-of-range")).quote("SPY"),
            Err(BrokerError::InvalidFixtureData { .. })
        ));
    }

    #[test]
    fn symbols_require_an_ascii_alphanumeric_character() {
        assert!(!valid_symbol("---"));
        assert!(!valid_symbol("///"));
        assert!(valid_symbol("BTC/USD"));
    }

    #[test]
    fn reads_account_and_builds_order_preview_without_submission() {
        let broker = broker();
        assert_eq!(
            broker.account_summary().unwrap().account_id,
            "fixture-account"
        );

        let preview = broker
            .preview_order(&OrderRequest {
                symbol: " spy ".into(),
                side: Side::Buy,
                kind: OrderKind::Market,
                quantity: 2.0,
                limit_price: None,
            })
            .unwrap();
        assert_eq!(preview.request.symbol, "SPY");
        assert_eq!(preview.reference_price, 540.25);
        assert_eq!(preview.estimated_notional, 1080.5);
    }

    #[test]
    fn validates_orders_before_preview() {
        let mut request = OrderRequest {
            symbol: "SPY".into(),
            side: Side::Buy,
            kind: OrderKind::Limit,
            quantity: 1.0,
            limit_price: None,
        };
        assert!(matches!(
            broker().preview_order(&request),
            Err(BrokerError::InvalidOrder(
                model::OrderValidationError::MissingLimitPrice
            ))
        ));

        request.kind = OrderKind::Market;
        request.quantity = f64::NAN;
        assert!(matches!(
            broker().preview_order(&request),
            Err(BrokerError::InvalidOrder(
                model::OrderValidationError::InvalidQuantity
            ))
        ));
    }

    #[test]
    fn rejects_limit_notional_underflow_during_validation() {
        let request = OrderRequest {
            symbol: "SPY".into(),
            side: Side::Buy,
            kind: OrderKind::Limit,
            quantity: f64::from_bits(1),
            limit_price: Some(f64::from_bits(1)),
        };
        assert!(matches!(
            broker().preview_order(&request),
            Err(BrokerError::InvalidOrder(
                model::OrderValidationError::InvalidNotional
            ))
        ));
    }

    #[test]
    fn rejects_market_notional_overflow_during_preview() {
        let request = OrderRequest {
            symbol: "SPY".into(),
            side: Side::Buy,
            kind: OrderKind::Market,
            quantity: f64::MAX,
            limit_price: None,
        };
        assert!(matches!(
            broker().preview_order(&request),
            Err(BrokerError::PreviewValueOutOfRange)
        ));
    }
}
