use crate::iperf_reports::TestResults;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct TestRequest {
    pub client_name: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct TestReservation {
    pub port_number: u16,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct PubTestReport {
    pub report: TestResults,
}
