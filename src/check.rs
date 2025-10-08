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

pub trait ValidateWithExt {
    fn validate_with(self, validate: fn(&str) -> Result<String, ItemError>) -> ProcessingResult;
}

impl ValidateWithExt for Vec<&str> {
    fn validate_with(self, validate: fn(&str) -> Result<String, ItemError>) -> ProcessingResult {
        let (valid, invalid): (Vec<String>, Vec<ItemError>) = self
            .into_iter()
            .map(validate)
            .partition_map(|result| match result {
                Ok(valid_item) => Either::Left(valid_item),
                Err(error) => Either::Right(error),
            });
        ProcessingResult { invalid, valid }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn is_even_validator(s: &str) -> Result<String, ItemError> {
        match s.parse::<i32>() {
            Ok(num) if num % 2 == 0 => Ok(num.to_string()),
            Ok(_) => Err(ItemError {
                item: s.to_string(),
                message: "Odd number".to_string(),
            }),
            Err(_) => Err(ItemError {
                item: s.to_string(),
                message: "Not a number".to_string(),
            }),
        }
    }

    #[test]
    fn test_validate_with_all_valid() {
        let items = vec!["2", "4", "6"];
        let result = items.validate_with(is_even_validator);
        assert_eq!(
            result.valid,
            vec!["2".to_string(), "4".to_string(), "6".to_string()]
        );
        assert_eq!(result.invalid.len(), 0);
    }

    #[test]
    fn test_validate_with_all_invalid() {
        let items = vec!["1", "3", "five"];
        let result = items.validate_with(is_even_validator);
        assert_eq!(result.valid.len(), 0);
        assert_eq!(result.invalid.len(), 3);
        assert_eq!(result.invalid[0].item, "1");
        assert_eq!(result.invalid[0].message, "Odd number");
        assert_eq!(result.invalid[1].item, "3");
        assert_eq!(result.invalid[1].message, "Odd number");
        assert_eq!(result.invalid[2].item, "five");
        assert_eq!(result.invalid[2].message, "Not a number");
    }

    #[test]
    fn test_validate_with_mixed() {
        let items = vec!["2", "1", "4", "three", "6"];
        let result = items.validate_with(is_even_validator);
        assert_eq!(
            result.valid,
            vec!["2".to_string(), "4".to_string(), "6".to_string()]
        );
        assert_eq!(result.invalid.len(), 2);
        assert_eq!(result.invalid[0].item, "1");
        assert_eq!(result.invalid[0].message, "Odd number");
        assert_eq!(result.invalid[1].item, "three");
        assert_eq!(result.invalid[1].message, "Not a number");
    }

    #[test]
    fn test_validate_with_empty() {
        let items: Vec<&str> = vec![];
        let result = items.validate_with(is_even_validator);
        assert_eq!(result.valid.len(), 0);
        assert_eq!(result.invalid.len(), 0);
    }

    #[test]
    fn test_item_error_display() {
        let error = ItemError {
            item: "foo".to_string(),
            message: "bar".to_string(),
        };
        assert_eq!(format!("{}", error), "Invalid item: foo - bar");
    }

    #[test]
    fn test_partition_map_ext() {
        let iter = (1..6).map(|x| {
            if x % 2 == 0 {
                Either::Left(x)
            } else {
                Either::Right(x)
            }
        });
        let (left, right): (Vec<i32>, Vec<i32>) = iter.partition_map(|e| e);
        assert_eq!(left, vec![2, 4]);
        assert_eq!(right, vec![1, 3, 5]);
    }
}
