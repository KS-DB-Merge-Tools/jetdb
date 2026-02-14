use jetdb::ddl::{self, DdlDialect};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum DdlFormat {
    Sqlite,
    Postgres,
    Mysql,
    Access,
}

pub fn create_dialect(format: DdlFormat) -> Box<dyn DdlDialect> {
    match format {
        DdlFormat::Sqlite => Box::new(ddl::Sqlite),
        DdlFormat::Postgres => Box::new(ddl::Postgres),
        DdlFormat::Mysql => Box::new(ddl::Mysql),
        DdlFormat::Access => Box::new(ddl::Access),
    }
}
