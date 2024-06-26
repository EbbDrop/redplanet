use gdbstub::conn::Connection;

pub struct TcpStream(pub tokio::net::TcpStream);

impl Connection for TcpStream {
    type Error = tokio::io::Error;

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.0.try_write(&[byte]).map(|_| ())
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.0.try_write(buf).map(|_| ())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // Can't do sync flush without asyn
        Ok(())
    }

    fn on_session_start(&mut self) -> Result<(), Self::Error> {
        self.0.set_nodelay(true)
    }
}
