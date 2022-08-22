#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    error::Error,
    fs::File,
    io::{self, Read, Write},
};

use clap::Parser;
use csv::Trim;
use rust_decimal::Decimal;
use serde::Deserialize;

/// A client ID
type ClientId = u16;
/// A transaction ID
type TransactionId = u32;
/// An amount of money
/// We use a fixed-point decimal number here and not a floating-point one to
/// prevent any rounding issue and loss of precision as we are in a financial
/// context
/// The performance cost is negligible compared to the impact of a loss in
/// precision
type MoneyAmount = Decimal;

const DECIMAL_PRECISION: u32 = 4;

/// Account data for a client
#[derive(Default, PartialEq, Debug)]
struct Client {
    /// Available funds
    available_funds: MoneyAmount,
    /// Held funds
    held_funds: MoneyAmount,
    /// Is this account locked?
    is_locked: bool,
}

impl Client {
    /// Sum of available and held funds
    fn total_funds(&self) -> MoneyAmount {
        self.available_funds + self.held_funds
    }
}

/// The various states of a disputed transaction
#[derive(PartialEq)]
enum DisputedState {
    NotDisputed,
    Disputed,
    Resolved,
    ChargedBack,
}

impl Default for DisputedState {
    fn default() -> Self {
        DisputedState::NotDisputed
    }
}

/// A transaction
struct Transaction {
    /// The amount of money that has been deposited or withdrawn
    amount: MoneyAmount,
    /// The disputed state of this transaction
    disputed: DisputedState,
}

/// An entry in the transaction input
#[derive(Deserialize)]
struct TransactionRecord {
    /// A string representing the transaction type
    #[serde(rename = "type")]
    type_string: String,
    /// The client ID that has triggered this transaction
    #[serde(rename = "client")]
    client_id: ClientId,
    /// The transaction ID can either be the ID of the current transaction, or
    /// the ID of a target transaction (dispute, resolve, chargeback)
    #[serde(rename = "tx")]
    id: TransactionId,
    /// An amount related to this transaction
    amount: Option<MoneyAmount>,
}

impl TransactionRecord {
    fn to_transaction(self) -> Transaction {
        Transaction {
            amount: self.amount.unwrap(), // we unwrap here since we know that
            // this transaction has already been processed before, so any
            // missing amount entry would have been caught there.
            disputed: DisputedState::default(),
        }
    }
}

#[derive(Parser)]
#[clap(name = "Rust Payments Challenge")]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// File containing the transactions to process
    transactions_filepath: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let file = File::open(args.transactions_filepath)?;
    let clients = process_transactions(file)?;
    write_result(clients, io::stdout())?;
    Ok(())
}

// Process a deposit
fn process_deposit(client: &mut Client, amount: Option<MoneyAmount>) -> Result<(), Box<dyn Error>> {
    if let Some(amount) = amount {
        client.available_funds += amount;
    } else {
        Err("deposit without amount")?
    }
    Ok(())
}

// Process a withdrawal
fn process_withdrawal(
    client: &mut Client,
    amount: Option<MoneyAmount>,
) -> Result<(), Box<dyn Error>> {
    if let Some(amount) = amount {
        if client.available_funds < amount {
            Err("not enough available funds")?
        }
        client.available_funds -= amount;
    } else {
        Err("withdrawal without amount")?
    }
    Ok(())
}

// Process a dispute
fn process_dispute(
    client: &mut Client,
    transaction_id: TransactionId,
    transactions: &mut HashMap<TransactionId, Transaction>,
) -> Result<(), Box<dyn Error>> {
    if let Some(target_transaction) = transactions.get_mut(&transaction_id) {
        if target_transaction.disputed != DisputedState::NotDisputed {
            Err("transaction is already under dispute")?
        }
        client.held_funds += target_transaction.amount;
        client.available_funds -= target_transaction.amount;
        target_transaction.disputed = DisputedState::Disputed;
    } else {
        Err("unknown transaction id")?
    }
    Ok(())
}

// Process a resolve
fn process_resolve(
    client: &mut Client,
    transaction_id: TransactionId,
    transactions: &mut HashMap<TransactionId, Transaction>,
) -> Result<(), Box<dyn Error>> {
    if let Some(target_transaction) = transactions.get_mut(&transaction_id) {
        if target_transaction.disputed != DisputedState::Disputed {
            Err("transaction is not under dispute")?
        }
        client.held_funds -= target_transaction.amount;
        client.available_funds += target_transaction.amount;
        target_transaction.disputed = DisputedState::Resolved;
    } else {
        Err("unknown transaction id")?
    }
    Ok(())
}

// Process a chargeback
fn process_chargeback(
    client: &mut Client,
    transaction_id: TransactionId,
    transactions: &mut HashMap<TransactionId, Transaction>,
) -> Result<(), Box<dyn Error>> {
    if let Some(target_transaction) = transactions.get_mut(&transaction_id) {
        if target_transaction.disputed != DisputedState::Disputed {
            Err("transaction is not under dispute")?
        }
        client.held_funds -= target_transaction.amount;
        client.is_locked = true;
        target_transaction.disputed = DisputedState::ChargedBack;
    } else {
        Err("unknown transaction id")?
    }
    Ok(())
}

// Process a transaction
fn process_transaction(
    record: TransactionRecord,
    transactions: &mut HashMap<TransactionId, Transaction>,
    clients: &mut HashMap<ClientId, Client>,
) -> Result<(), Box<dyn Error>> {
    if let Some(amount) = record.amount {
        if amount.is_sign_negative() {
            Err("amount must be a positive number")?
        }
        if amount.is_zero() {
            Err("amount must be greater than zero")?
        }
    }
    // Return a client for this id; create a new one if none is found
    // We assume clients start with an empty account
    let client = clients.entry(record.client_id).or_default();
    // Refuse to process transactions for locked client accounts
    if client.is_locked {
        Err("client account is locked")?
    }
    // Note that we only store deposits and withdrawals, as other transaction
    // types don't need to be stored and are processed on the fly
    match record.type_string.as_str() {
        // A deposit; a credit to the client's asset account
        "deposit" => {
            process_deposit(client, record.amount)?;
            // Only store successful deposits
            transactions.insert(record.id, record.to_transaction());
        }
        // A withdrawal; a debit to the client's asset account
        "withdrawal" => {
            process_withdrawal(client, record.amount)?;
            // Only store successful withdrawals
            transactions.insert(record.id, record.to_transaction());
        }
        // A dispute: claim that a transaction was erroneous
        "dispute" => process_dispute(client, record.id, transactions)?,
        // A resolve: resolution to a dispute
        "resolve" => process_resolve(client, record.id, transactions)?,
        // A chargeback: client reversing a transaction
        "chargeback" => process_chargeback(client, record.id, transactions)?,
        _ => Err("unknown transaction type")?,
    }
    Ok(())
}

/// Reads the transactions from a reader and processes them
/// We could have split this function into two: reading and processing but it is
/// more efficient to process the transactions on the fly rather than storing
/// all of them first
/// This function returns a map of all clients
fn process_transactions<R: Read>(reader: R) -> Result<HashMap<ClientId, Client>, Box<dyn Error>> {
    let mut clients: HashMap<ClientId, Client> = HashMap::new();
    let mut transactions: HashMap<TransactionId, Transaction> = HashMap::new();
    let mut reader = csv::ReaderBuilder::new()
        .trim(Trim::All) // ignore spaces/tabs
        .flexible(true) // allow missing fields (amount for instance)
        .from_reader(reader);
    for record in reader.deserialize() {
        let transaction_record: TransactionRecord = record?;
        // Transaction processing errors are not fatal
        if let Err(err) = process_transaction(transaction_record, &mut transactions, &mut clients) {
            eprintln!("Error processing transaction: {}", err);
        }
    }
    Ok(clients)
}

/// Writes the client's account status to a writer
fn write_result<W: Write>(
    clients: HashMap<ClientId, Client>,
    writer: W,
) -> Result<(), Box<dyn Error>> {
    let mut wtr = csv::Writer::from_writer(writer);
    wtr.write_record(&["client", "available", "held", "total", "locked"])?;
    for (id, client) in clients {
        wtr.serialize((
            id,
            client.available_funds.round_dp(DECIMAL_PRECISION),
            client.held_funds.round_dp(DECIMAL_PRECISION),
            client.total_funds().round_dp(DECIMAL_PRECISION),
            client.is_locked,
        ))?;
    }
    wtr.flush()?;
    Ok(())
}
