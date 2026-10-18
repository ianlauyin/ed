pub trait Domain: 'static {
    const NAME: &str;
    /// Statements creating this domain's tables. There are no migrations: changing this
    /// discards the whole database.
    const SCHEMA: &str;
}

/// The full set of schema statements a database is created from.
pub trait Schema: 'static {
    fn statements() -> Vec<&'static str>;
}

impl Schema for () {
    fn statements() -> Vec<&'static str> {
        Vec::new()
    }
}

impl<D: Domain> Schema for D {
    fn statements() -> Vec<&'static str> {
        vec![D::SCHEMA]
    }
}
