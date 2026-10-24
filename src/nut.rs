//! Client for the NUT network protocol: https://networkupstools.org/docs/developer-guide.chunked/net-protocol.html

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, Lines};
use tokio::net::TcpStream;

/// Everything read from a NUT server in one scrape.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Scrape {
    pub nut_version: String,
    /// By UPS name, sorted for a stable output
    pub upses: BTreeMap<String, Ups>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Ups {
    pub description: String,
    pub vars: HashMap<String, String>,
}

#[derive(Debug)]
pub enum Error {
    Connect(io::Error),
    Timeout,
    Io(io::Error),
    NotNut,
    /// `ERR <message>` from the server
    Server {
        query: String,
        message: String,
    },
    Malformed {
        query: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connect(err) => write!(f, "Failed to connect to target: {err}"),
            Error::Timeout => write!(f, "Failed to communicate with target: timed out"),
            Error::Io(err) => write!(f, "Failed to communicate with target: {err}"),
            Error::NotNut => {
                write!(f, "Failed to communicate with target: no NUT version in the answer. Not a NUT server?")
            }
            Error::Server { query, message } => {
                write!(f, "Failed to communicate with target: received error for query \"{query}\": {message}")
            }
            Error::Malformed { query } => {
                write!(f, "Failed to communicate with target: malformed list for query \"{query}\".")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

/// Connects to `target` (`host:port`) and reads all UPSes and their variables, within `timeout`.
pub async fn scrape_target(target: &str, timeout: Duration) -> Result<Scrape, Error> {
    let result = tokio::time::timeout(timeout, async {
        log::trace!("Connecting to NUT server: {target}");
        let stream = TcpStream::connect(target).await.map_err(Error::Connect)?;
        scrape(stream).await
    })
    .await;
    result.unwrap_or(Err(Error::Timeout))
}

pub async fn scrape<S: AsyncRead + AsyncWrite + Unpin>(stream: S) -> Result<Scrape, Error> {
    let mut client = Client::new(stream);
    let mut scrape = Scrape { nut_version: client.version().await?, upses: BTreeMap::new() };

    for words in client.list("LIST UPS").await? {
        // UPS <name> <description>
        match words.as_slice() {
            [kind, name, description] if kind == "UPS" => {
                let ups = Ups { description: description.clone(), vars: HashMap::new() };
                scrape.upses.insert(name.clone(), ups);
            }
            _ => return Err(Error::Malformed { query: "LIST UPS".into() }),
        }
    }

    for (name, ups) in &mut scrape.upses {
        let query = format!("LIST VAR {name}");
        for words in client.list(&query).await? {
            // VAR <ups> <variable> <value>
            match words.as_slice() {
                [kind, _, var, value] if kind == "VAR" => {
                    ups.vars.insert(var.clone(), value.clone());
                }
                _ => return Err(Error::Malformed { query }),
            }
        }
    }

    client.logout().await;
    Ok(scrape)
}

struct Client<S> {
    lines: Lines<BufReader<S>>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Client<S> {
    fn new(stream: S) -> Self {
        Client { lines: BufReader::new(stream).lines() }
    }

    async fn send(&mut self, query: &str) -> io::Result<()> {
        log::trace!("NUT query sent: {query}");
        let stream = self.lines.get_mut().get_mut();
        stream.write_all(format!("{query}\n").as_bytes()).await?;
        stream.flush().await
    }

    async fn next_line(&mut self) -> io::Result<Option<String>> {
        let line = self.lines.next_line().await?;
        if let Some(line) = &line {
            log::trace!("NUT query received: {line}");
        }
        Ok(line)
    }

    /// `VER`: "Network UPS Tools upsd <version> - http://www.networkupstools.org/"
    async fn version(&mut self) -> Result<String, Error> {
        self.send("VER").await?;
        let line = self.next_line().await?.unwrap_or_default();
        parse_version(&line).map(str::to_owned).ok_or(Error::NotNut)
    }

    /// Sends a `LIST` query and returns the words of each line between `BEGIN` and `END`.
    async fn list(&mut self, query: &str) -> Result<Vec<Vec<String>>, Error> {
        let malformed = || Error::Malformed { query: query.to_owned() };
        self.send(query).await?;

        let mut begun = false;
        let mut items = Vec::new();
        while let Some(line) = self.next_line().await? {
            if line.is_empty() {
                continue;
            }
            if let Some(message) = line.strip_prefix("ERR ") {
                return Err(Error::Server { query: query.to_owned(), message: message.to_owned() });
            }
            if line.starts_with("BEGIN ") {
                if begun {
                    return Err(malformed());
                }
                begun = true;
            } else if line.starts_with("END ") {
                return if begun { Ok(items) } else { Err(malformed()) };
            } else if begun {
                items.push(split_words(&line).ok_or_else(malformed)?);
            } else {
                return Err(malformed());
            }
        }
        // Connection closed before END
        Err(malformed())
    }

    /// Ends the session politely; the scrape already succeeded, so errors are ignored.
    async fn logout(&mut self) {
        if self.send("LOGOUT").await.is_ok() {
            let _ = self.next_line().await;
        }
    }
}

fn parse_version(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once("upsd ")?;
    let (version, _) = rest.rsplit_once(" -")?;
    (!version.is_empty()).then_some(version)
}

/// Splits a line into words: separated by spaces, or quoted with `"` and backslash escapes.
/// Returns `None` for an unterminated quote.
fn split_words(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_ascii_whitespace()).is_some() {}
        let Some(first) = chars.next() else { return Some(words) };
        let mut word = String::new();
        if first == '"' {
            loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => word.push(chars.next()?),
                    c => word.push(c),
                }
            }
        } else {
            word.push(first);
            while let Some(c) = chars.next_if(|c| !c.is_ascii_whitespace()) {
                word.push(c);
            }
        }
        words.push(word);
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, duplex};

    use super::*;

    /// At the end of an answer, makes the fake server close the connection after sending it
    const CLOSE: &str = "<close>";

    /// Runs `scrape` against a fake server that answers each query from `answers`.
    async fn scrape_with(answers: &[(&str, &str)]) -> Result<Scrape, Error> {
        let (client, mut server) = duplex(64 * 1024);
        let answers: Vec<(String, String)> = answers.iter().map(|(q, a)| (q.to_string(), a.to_string())).collect();
        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut chunk = [0u8; 1024];
            while let Ok(n) = server.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                buffer.push_str(std::str::from_utf8(&chunk[..n]).unwrap());
                while let Some((query, rest)) = buffer.split_once('\n') {
                    let answer = answers.iter().find(|(q, _)| q == query).map_or("ERR UNKNOWN-COMMAND\n", |(_, a)| a);
                    if let Some(answer) = answer.strip_suffix(CLOSE) {
                        server.write_all(answer.as_bytes()).await.unwrap();
                        return;
                    }
                    server.write_all(answer.as_bytes()).await.unwrap();
                    buffer = rest.to_owned();
                }
            }
        });
        // A test waiting for an answer that never comes fails instead of hanging
        tokio::time::timeout(Duration::from_secs(5), scrape(client)).await.expect("scrape timed out")
    }

    const VER: (&str, &str) = ("VER", "Network UPS Tools upsd 2.8.2 - https://www.networkupstools.org/\n");

    #[tokio::test]
    async fn reads_upses_and_vars() {
        let scrape = scrape_with(&[
            VER,
            ("LIST UPS", "BEGIN LIST UPS\nUPS b \"second\"\nUPS a \"first\"\nEND LIST UPS\n"),
            (
                "LIST VAR a",
                "BEGIN LIST VAR a\nVAR a battery.charge \"100\"\nVAR a ups.status \"OL CHRG\"\nEND LIST VAR a\n",
            ),
            ("LIST VAR b", "BEGIN LIST VAR b\nEND LIST VAR b\n"),
            ("LOGOUT", "OK Goodbye\n"),
        ])
        .await
        .unwrap();

        assert_eq!(scrape.nut_version, "2.8.2");
        assert_eq!(scrape.upses.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(scrape.upses["a"].description, "first");
        assert_eq!(scrape.upses["a"].vars["ups.status"], "OL CHRG");
        assert!(scrape.upses["b"].vars.is_empty());
    }

    #[tokio::test]
    async fn server_error_is_reported() {
        let err = scrape_with(&[VER, ("LIST UPS", "ERR ACCESS-DENIED\n")]).await.unwrap_err();
        assert!(matches!(&err, Error::Server { message, .. } if message == "ACCESS-DENIED"), "{err}");
    }

    #[tokio::test]
    async fn list_without_begin_is_malformed() {
        let err = scrape_with(&[VER, ("LIST UPS", "UPS a \"\"\nEND LIST UPS\n")]).await.unwrap_err();
        assert!(matches!(err, Error::Malformed { .. }), "{err}");
    }

    #[tokio::test]
    async fn connection_closed_before_end_is_malformed() {
        let err = scrape_with(&[VER, ("LIST UPS", "BEGIN LIST UPS\nUPS a \"\"\n<close>")]).await.unwrap_err();
        assert!(matches!(err, Error::Malformed { .. }), "{err}");
    }

    #[tokio::test]
    async fn not_a_nut_server() {
        let err = scrape_with(&[("VER", "SSH-2.0-OpenSSH_9.6\n")]).await.unwrap_err();
        assert!(matches!(err, Error::NotNut), "{err}");
    }

    #[test]
    fn version_formats() {
        assert_eq!(parse_version(include_str!("../tests/fixtures/VER-1.txt").trim()), Some("2.7.4"));
        assert_eq!(parse_version(include_str!("../tests/fixtures/VER-2.txt").trim()), Some("DSM6-2-25510-201118"));
        assert_eq!(parse_version("Network UPS Tools upsd  - x"), None);
    }

    #[test]
    fn words() {
        assert_eq!(
            split_words(r#"VAR ups-1 device.model "Back-UPS ES 700""#).unwrap(),
            ["VAR", "ups-1", "device.model", "Back-UPS ES 700"]
        );
        assert_eq!(split_words(r#"VAR a b "say \"hi\" \\o/""#).unwrap(), ["VAR", "a", "b", r#"say "hi" \o/"#]);
        assert_eq!(split_words(r#"UPS a """#).unwrap(), ["UPS", "a", ""]);
        assert_eq!(split_words("  a   b ").unwrap(), ["a", "b"]);
        assert_eq!(split_words(r#"VAR a b "unterminated"#), None);
    }
}
