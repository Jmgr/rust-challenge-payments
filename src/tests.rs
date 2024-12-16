use super::*;
use rust_decimal_macros::dec;

// Tests that invalid input returns an error
#[test]
fn test_invalid_input() {
    let input = r#"invalid
	input"#;
    let result = process_transactions(input.as_bytes());
    assert!(result.is_err());
}

// Tests that a few deposits return the expected result
#[test]
fn test_deposits() -> Result<(), Error> {
    let input = r#"type, client, tx, amount
	deposit, 1, 1, 1.0
	deposit, 2, 2, 2.0
	deposit, 1, 3, 2.0"#;
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 2);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(3).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );
    assert_eq!(
        result.get(&ClientId(2)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );

    Ok(())
}

// Test that deposits with invalid amounts are ignored
#[test]
fn test_invalid_deposits() -> Result<(), Error> {
    let input = r#"type, client, tx, amount
	deposit, 1, 1, -1.0
	deposit, 2, 2, 2.0
	deposit, 1, 3, 2.0"#;
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 2);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );
    assert_eq!(
        result.get(&ClientId(2)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );

    let input = r#"type, client, tx, amount
	deposit, 1, 1, 0.0
	deposit, 2, 2, 2.0
	deposit, 1, 3, 2.0"#;
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 2);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );
    assert_eq!(
        result.get(&ClientId(2)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );

    let input = r#"type, client, tx, amount
	deposit, 1, 1
	deposit, 2, 2, 2.0
	deposit, 1, 3, 2.0"#;
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 2);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );
    assert_eq!(
        result.get(&ClientId(2)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );

    Ok(())
}

// Tests that a deposits and withdrawals return the expected result
#[test]
fn test_withdrawals() -> Result<(), Error> {
    let input = r#"type, client, tx, amount
	deposit, 1, 1, 1.0
	deposit, 2, 2, 2.0
	deposit, 1, 3, 2.0
	withdrawal, 1, 4, 1.5
	withdrawal, 2, 5, 3.0"#;
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 2);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(1.5).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );
    assert_eq!(
        result.get(&ClientId(2)).unwrap(),
        &Client {
            available_funds: dec!(2).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );

    Ok(())
}

// Tests a dispute and a resolve; try various invalid transactions and check
// that they are ignored
#[test]
fn test_dispute_and_resolve() -> Result<(), Error> {
    let input = r#"type, client, tx, amount
    deposit,    1, 1,  2.0
    resolve,    1, 1
    withdrawal, 1, 2,  1.5
    dispute,    1, 2
    resolve,    1, 2
    dispute,    1, 2
    deposit,    1, 10, 2.0"#;
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 1);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(2.5).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );

    let input = r#"type, client, tx, amount
	deposit,    1, 1,  2.0
	dispute,    1, 1
	resolve,    1, 1
	dispute,    1, 2
	deposit,    1, 10, 2.0"#;
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 1);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(4).into(),
            held_funds: dec!(0).into(),
            is_locked: false,
        }
    );

    Ok(())
}

// Tests a dispute and a chargeback
#[test]
fn test_dispute_and_chargeback() -> Result<(), Error> {
    let input = r#"type, client, tx, amount
	deposit,    1, 1,  2.0
	withdrawal, 1, 2,  1.5
	dispute,    1, 2
	chargeback, 1, 2
	deposit,    1, 10, 2.0"#; // This won't be allowed since the account has been frozen
    let result = process_transactions(input.as_bytes())?;
    assert_eq!(result.len(), 1);
    assert_eq!(
        result.get(&ClientId(1)).unwrap(),
        &Client {
            available_funds: dec!(-1).into(),
            held_funds: dec!(0).into(),
            is_locked: true,
        }
    );

    Ok(())
}
