use std::fmt;

#[derive(Debug)]
pub struct ItemError {
    pub item: String,
    pub message: String,
}

#[derive(Debug)]
pub struct ProcessingResult {
    pub invalid: Vec<ItemError>,
    pub valid: Vec<String>,
}

pub fn validate_with(
    lns: Vec<&str>,
    validate: fn(&str) -> Result<String, ItemError>,
) -> ProcessingResult {
    let (valid, invalid): (Vec<String>, Vec<ItemError>) = lns
        .into_iter()
        .map(validate)
        .partition_map(|result| match result {
            Ok(valid_item) => Either::Left(valid_item),
            Err(error) => Either::Right(error),
        });
    ProcessingResult { invalid, valid }
}

enum Either<L, R> {
    Left(L),
    Right(R),
}

trait PartitionMapExt: Iterator + Sized {
    fn partition_map<F, L, R>(self, mut f: F) -> (Vec<L>, Vec<R>)
    where
        F: FnMut(Self::Item) -> Either<L, R>,
    {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        self.for_each(|item| match f(item) {
            Either::Left(l) => left.push(l),
            Either::Right(r) => right.push(r),
        });
        (left, right)
    }
}

impl<I: Iterator> PartitionMapExt for I {}

impl fmt::Display for ItemError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Invalid item: {} - {}", self.item, self.message)
    }
}
