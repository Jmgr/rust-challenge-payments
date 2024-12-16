#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![allow(clippy::enum_variant_names)] // Allow "Error" suffix in the Error enum

#[cfg(test)]
mod tests;

use clap::Parser;
use csv::Trim;
use derive_more::{Add, AddAssign, Display, SubAssign};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Write},
};
use thiserror::Error;

/// Any error that can be triggered by this application.
#[derive(Debug, Error)]
enum Error {
    #[error("failed reading transaction file {0}: {1}")]
    TransactionFileReadError(PathBuf, io::Error),

    #[error("write error: {0}")]
    WriteError(csv::Error),

    #[error("flush error: {0}")]
    FlushError(io::Error),

    #[error("serialization error: {0}")]
    SerializationError(csv::Error),

    #[error("failed parsing transaction: {0}")]
    ParsingError(csv::Error),

    #[error("deposit without amount")]
    DepositWithoutAmount,

    #[error("withdrawal without amount")]
    WithdrawalWithoutAmount,

    #[error("transaction without amount")]
    TransactionWithoutAmount,

    #[error("unknown transaction ID: {0}")]
    UnknownTransactionId(TransactionId),

    #[error("client {0}: withdrawal without enough available funds, needed {1}, available {2}")]
    NotEnoughAvailableFunds(ClientId, MoneyAmount, MoneyAmount),

    #[error("transaction {0} already under dispute")]
    TransactionAlreadyUnderDispute(TransactionId),

    #[error("transaction {0} not under dispute")]
    TransactionNotUnderDispute(TransactionId),

    #[error("amount must be greater than zero")]
    InvalidAmount(MoneyAmount),

    #[error("client account {0} is locked")]
    ClientLocked(ClientId),

    #[error("unknown transaction type: {0}")]
    UnknownTransactionType(String),
}

/// A client ID.
#[derive(Clone, Copy, Debug, Deserialize, Display, Eq, Hash, PartialEq, Serialize)]

struct ClientId(u16);

/// A transaction ID.
#[derive(Clone, Copy, Debug, Deserialize, Display, Eq, Hash, PartialEq)]
struct TransactionId(u32);

/// An amount of money.
/// We use a fixed-point decimal number here and not a floating-point one to
/// prevent any rounding issue and loss of precision as we are in a financial
/// context.
/// The performance cost is negligible compared to the impact of a loss in
/// precision.
#[derive(
    Add,
    AddAssign,
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    Display,
    PartialEq,
    PartialOrd,
    SubAssign,
)]
struct MoneyAmount(Decimal);

/// We implement Deref and DerefMut here for convenience, so that Decimal functions can be called
/// directly. We could instead provide only access to a selection of functions if wanted.
impl Deref for MoneyAmount {
    type Target = Decimal;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MoneyAmount {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// From trait to convert a Decimal into a MoneyAmount.
/// We only implement this for tests because it could be too risky to allow converting a Decimal to
/// a MoneyAmount implicitly by only calling "into()".
/// In production code we expect users to explicitly do the conversion.
#[cfg(test)]
impl From<Decimal> for MoneyAmount {
    fn from(value: Decimal) -> Self {
        Self(value)
    }
}

const DECIMAL_PRECISION: u32 = 4;

/// Account data for a client.
#[derive(Debug, Default, PartialEq)]
struct Client {
    /// Available funds.
    available_funds: MoneyAmount,
    /// Held funds.
    held_funds: MoneyAmount,
    /// Is this account locked?
    is_locked: bool,
}

impl Client {
    /// Sum of available and held funds.
    fn total_funds(&self) -> MoneyAmount {
        self.available_funds + self.held_funds
    }
}

/// The various states of a disputed transaction.
#[derive(Debug, Default, PartialEq, Display)]
enum DisputedState {
    /// This transaction is not disputed.
    #[default]
    NotDisputed,

    /// This is a disputed transaction.
    Disputed,

    /// This transaction has been resolved.
    Resolved,

    /// This transaction has been charged back.
    ChargedBack,
}

#[derive(Debug)]
/// A transaction.
struct Transaction {
    /// The amount of money that has been deposited or withdrawn.
    amount: MoneyAmount,
    /// The disputed state of this transaction.
    disputed: DisputedState,
}

/// An entry in the transaction input.
#[derive(Debug, Deserialize)]
struct TransactionRecord {
    /// A string representing the transaction type.
    #[serde(rename = "type")]
    type_string: String,
    /// The client ID that has triggered this transaction.
    #[serde(rename = "client")]
    client_id: ClientId,
    /// The transaction ID can either be the ID of the current transaction, or
    /// the ID of a target transaction (dispute, resolve, chargeback).
    #[serde(rename = "tx")]
    id: TransactionId,
    /// An amount related to this transaction.
    amount: Option<MoneyAmount>,
}

impl TryFrom<TransactionRecord> for Transaction {
    type Error = Error;

    fn try_from(transaction_record: TransactionRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            amount: transaction_record
                .amount
                .ok_or(Error::TransactionWithoutAmount)?,
            disputed: DisputedState::default(),
        })
    }
}

#[derive(Parser)]
#[clap(name = "Rust Payments Challenge")]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// File containing the transactions to process.
    transactions_filepath: PathBuf,
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    let file = File::open(&args.transactions_filepath)
        .map_err(|err| Error::TransactionFileReadError(args.transactions_filepath, err))?;
    let clients = process_transactions(file)?;

    write_result(clients, io::stdout())?;

    Ok(())
}

/// Process a deposit.
fn process_deposit(client: &mut Client, amount: Option<MoneyAmount>) -> Result<(), Error> {
    let Some(amount) = amount else {
        return Err(Error::DepositWithoutAmount);
    };

    client.available_funds += amount;

    Ok(())
}

/// Process a withdrawal.
fn process_withdrawal(client: &mut Client, client_id: ClientId, amount: Option<MoneyAmount>) -> Result<(), Error> {
    let Some(amount) = amount else {
        return Err(Error::WithdrawalWithoutAmount);
    };

    if client.available_funds < amount {
        return Err(Error::NotEnoughAvailableFunds(
            client_id,
            amount,
            client.available_funds,
        ));
    }

    client.available_funds -= amount;

    Ok(())
}

/// Process a dispute.
fn process_dispute(
    client: &mut Client,
    transaction_id: TransactionId,
    transactions: &mut HashMap<TransactionId, Transaction>,
) -> Result<(), Error> {
    let Some(target_transaction) = transactions.get_mut(&transaction_id) else {
        return Err(Error::UnknownTransactionId(transaction_id));
    };

    if target_transaction.disputed != DisputedState::NotDisputed {
        return Err(Error::TransactionAlreadyUnderDispute(transaction_id));
    }

    client.held_funds += target_transaction.amount;
    client.available_funds -= target_transaction.amount;
    target_transaction.disputed = DisputedState::Disputed;

    Ok(())
}

/// Process a resolve.
fn process_resolve(
    client: &mut Client,
    transaction_id: TransactionId,
    transactions: &mut HashMap<TransactionId, Transaction>,
) -> Result<(), Error> {
    let Some(target_transaction) = transactions.get_mut(&transaction_id) else {
        return Err(Error::UnknownTransactionId(transaction_id));
    };

    if target_transaction.disputed != DisputedState::Disputed {
        return Err(Error::TransactionNotUnderDispute(transaction_id));
    }

    client.held_funds -= target_transaction.amount;
    client.available_funds += target_transaction.amount;
    target_transaction.disputed = DisputedState::Resolved;

    Ok(())
}

/// Process a chargeback.
fn process_chargeback(
    client: &mut Client,
    transaction_id: TransactionId,
    transactions: &mut HashMap<TransactionId, Transaction>,
) -> Result<(), Error> {
    let Some(target_transaction) = transactions.get_mut(&transaction_id) else {
        return Err(Error::UnknownTransactionId(transaction_id));
    };

    if target_transaction.disputed != DisputedState::Disputed {
        return Err(Error::TransactionNotUnderDispute(transaction_id));
    }

    client.held_funds -= target_transaction.amount;
    client.is_locked = true;
    target_transaction.disputed = DisputedState::ChargedBack;

    Ok(())
}

/// Process a transaction.
fn process_transaction(
    record: TransactionRecord,
    transactions: &mut HashMap<TransactionId, Transaction>,
    clients: &mut HashMap<ClientId, Client>,
) -> Result<(), Error> {
    if let Some(amount) = record.amount {
        if amount.is_sign_negative() || amount.is_zero() {
            return Err(Error::InvalidAmount(amount));
        }
    }
    // Return a client for this id; create a new one if none is found
    // We assume clients start with an empty account
    let client = clients.entry(record.client_id).or_default();
    // Refuse to process transactions for locked client accounts
    if client.is_locked {
        return Err(Error::ClientLocked(record.client_id));
    }
    // Note that we only store deposits and withdrawals, as other transaction
    // types don't need to be stored and are processed on the fly
    match record.type_string.as_str() {
        // A deposit; a credit to the client's asset account
        "deposit" => {
            process_deposit(client, record.amount)?;
            // Only store successful deposits
            transactions.insert(record.id, record.try_into()?);
        }
        // A withdrawal; a debit to the client's asset account
        "withdrawal" => {
            process_withdrawal(client, record.client_id, record.amount)?;
            // Only store successful withdrawals
            transactions.insert(record.id, record.try_into()?);
        }
        // A dispute: claim that a transaction was erroneous
        "dispute" => process_dispute(client, record.id, transactions)?,
        // A resolve: resolution to a dispute
        "resolve" => process_resolve(client, record.id, transactions)?,
        // A chargeback: client reversing a transaction
        "chargeback" => process_chargeback(client, record.id, transactions)?,
        _ => return Err(Error::UnknownTransactionType(record.type_string)),
    }
    Ok(())
}

/// Reads the transactions from a reader and processes them.
/// We could have split this function into two: reading and processing, but it is
/// more efficient to process the transactions on the fly rather than storing
/// all of them first.
/// This function returns a map of all clients.
fn process_transactions<R: Read>(reader: R) -> Result<HashMap<ClientId, Client>, Error> {
    let mut clients = HashMap::new();
    let mut transactions = HashMap::new();
    let mut reader = csv::ReaderBuilder::new()
        .trim(Trim::All) // ignore spaces/tabs
        .flexible(true) // allow missing fields (amount for instance)
        .from_reader(reader);

    for record in reader.deserialize() {
        let transaction_record = record.map_err(Error::ParsingError)?;
        // Transaction processing errors are not fatal
        if let Err(err) = process_transaction(transaction_record, &mut transactions, &mut clients) {
            eprintln!("Error processing transaction: {}", err);
        }
    }

    Ok(clients)
}

/// Writes the client's account status to a writer.
fn write_result<W: Write>(clients: HashMap<ClientId, Client>, writer: W) -> Result<(), Error> {
    let mut writer = csv::Writer::from_writer(writer);
    writer.write_record(["client", "available", "held", "total", "locked"])
        .map_err(Error::WriteError)?;

    for (id, client) in clients {
        writer.serialize((
            id,
            client.available_funds.round_dp(DECIMAL_PRECISION),
            client.held_funds.round_dp(DECIMAL_PRECISION),
            client.total_funds().round_dp(DECIMAL_PRECISION),
            client.is_locked,
        ))
        .map_err(Error::SerializationError)?;
    }

    writer.flush().map_err(Error::FlushError)?;

    Ok(())
}
