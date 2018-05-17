#![deny(warnings)]

#[macro_use]
extern crate failure;
#[macro_use]
extern crate hyper;
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
extern crate serde;
#[macro_use]
extern crate log;
extern crate env_logger;
#[macro_use]
extern crate structopt;

use failure::Error;
use std::net::IpAddr;

#[derive(Deserialize, Debug)]
struct IpifyResponse {
    ip: IpAddr,
}

header! { (PddToken, "PddToken") => [String] }

fn real_ip() -> Result<IpAddr, Error> {
    let response: IpifyResponse = reqwest::get("https://api.ipify.org?format=json")?.json()?;
    let ip = response.ip;
    debug!("Real IP is {:?}", ip);
    Ok(ip)
}

#[derive(Deserialize, Debug)]
struct Record {
    #[serde(rename = "record_id")]
    id: i64,
    content: String,
    subdomain: String,
}

fn find_all_records(token: &str, domain: &str) -> Result<Vec<Record>, Error> {
    #[derive(Deserialize, Debug)]
    #[serde(tag = "success")]
    enum YandexListResponse {
        #[serde(rename = "ok")]
        Ok { records: Vec<Record> },
        #[serde(rename = "error")]
        Error { error: String },
    }
    let client = reqwest::Client::new();
    let response: YandexListResponse = client
        .get("https://pddimp.yandex.ru/api2/admin/dns/list")
        .header(PddToken(token.to_owned()))
        .query(&[("domain", domain)])
        .send()?
        .json()?;
    trace!("{:?}", response);
    match response {
        YandexListResponse::Ok { records } => Ok(records),
        YandexListResponse::Error { error } => bail!("Query failed: {}", error),
    }
}

fn find_record(token: &str, domain: &str, subdomain: &str) -> Result<Record, Error> {
    match find_all_records(token, domain)?
        .into_iter()
        .find(|record| record.subdomain == subdomain)
    {
        Some(record) => Ok(record),
        None => bail!("No record for subdomain {:?}", subdomain),
    }
}

fn current_ip(token: &str, domain: &str, subdomain: &str) -> Result<IpAddr, Error> {
    let ip: IpAddr = find_record(token, domain, subdomain)?.content.parse()?;
    debug!("Current IP is {:?}", ip);
    Ok(ip)
}

fn set_ip(token: &str, domain: &str, subdomain: &str, ip: IpAddr) -> Result<(), Error> {
    #[derive(Deserialize, Debug)]
    #[serde(tag = "success")]
    enum YandexUpdateResponse {
        #[serde(rename = "ok")]
        Ok { record: Record },
        #[serde(rename = "error")]
        Error { error: String },
    }
    let record_id = find_record(token, domain, subdomain)?.id;
    let client = reqwest::Client::new();
    let response: YandexUpdateResponse = client
        .post("https://pddimp.yandex.ru/api2/admin/dns/edit")
        .header(PddToken(token.to_owned()))
        .query(&[
            ("domain", domain),
            ("record_id", &record_id.to_string()),
            ("content", &ip.to_string()),
        ])
        .send()?
        .json()?;
    trace!("{:?}", response);
    match response {
        YandexUpdateResponse::Ok { record } => {
            info!("IP has been set to {:?}", record.content);
            Ok(())
        }
        YandexUpdateResponse::Error { error } => bail!("Update failed: {}", error),
    }
}

fn update(token: &str, domain: &str, subdomain: &str) -> Result<(), Error> {
    let current_ip = current_ip(token, domain, subdomain)?;
    let real_ip = real_ip()?;
    if current_ip != real_ip {
        info!(
            "Current {:?} differs from real {:?}, updating",
            current_ip, real_ip
        );
        set_ip(token, domain, subdomain, real_ip)?;
    } else {
        debug!("Current IP is same as real IP: {:?}", current_ip);
    }
    Ok(())
}

#[derive(StructOpt)]
#[structopt(name = "ypdddns", about = "Yandex PDD Dynamic DNS")]
enum Options {
    #[structopt(name = "set")]
    Set {
        token: String,
        domain: String,
        value: IpAddr,
    },
    #[structopt(name = "update")]
    Update { token: String, domain: String },
}

fn main() -> Result<(), Error> {
    env_logger::try_init()?;
    let options: Options = structopt::StructOpt::from_args();
    let (token, subdomain, domain) = match &options {
        Options::Set { token, domain, .. } | Options::Update { token, domain } => {
            let first_point = match domain.find('.') {
                Some(idx) => idx,
                None => bail!("domain doesn't contain '.'"),
            };
            (token, &domain[..first_point], &domain[first_point + 1..])
        }
    };
    debug!("domain={:?}, subdomain={:?}", domain, subdomain);
    match &options {
        Options::Set { value, .. } => {
            set_ip(token, domain, subdomain, *value)?;
        }
        Options::Update { .. } => {
            update(token, domain, subdomain)?;
        }
    }
    Ok(())
}
