use criterion::{criterion_group, criterion_main, Criterion};
use geozero::error::Result;
use geozero::Extent;
use geozero_core::geo::RustGeo;

mod fgb {
    use super::*;
    use flatgeobuf::*;
    use geozero::Extent;
    use std::fs::File;
    // seek_bufread::BufReader is much faster for bbox queries,
    // because seek resets buffer of std::io::BufReader
    use seek_bufread::BufReader;

    pub(super) fn fgb_to_geo(fpath: &str, bbox: &Option<Extent>, count: usize) -> Result<()> {
        let mut filein = BufReader::new(File::open(fpath)?);
        let mut fgb = FgbReader::open(&mut filein)?;
        if let Some(bbox) = bbox {
            fgb.select_bbox(bbox.minx, bbox.miny, bbox.maxx, bbox.maxy)?;
        } else {
            fgb.select_all()?;
        }
        let mut geo = RustGeo::new();
        fgb.process_features(&mut geo)?;
        assert_eq!(fgb.features_count(), count);
        Ok(())
    }

    pub(super) async fn fgb_http_to_geo(
        fname: &str,
        bbox: &Option<Extent>,
        count: usize,
    ) -> Result<()> {
        let url = format!("http://127.0.0.1:3333/{}", fname);
        let mut fgb = HttpFgbReader::open(&url).await?;
        if let Some(bbox) = bbox {
            fgb.select_bbox(bbox.minx, bbox.miny, bbox.maxx, bbox.maxy)
                .await?;
        } else {
            fgb.select_all().await?;
        }
        let mut geo = RustGeo::new();
        fgb.process_features(&mut geo).await?;
        assert_eq!(fgb.features_count(), count);
        Ok(())
    }
}

mod postgis_postgres {
    use geozero::Extent;
    use geozero_core::postgis::postgres::geo::Geometry as GeoZeroGeometry;
    use postgres::{self, Client, NoTls};

    // export DATABASE_URL=postgresql://pi@%2Fvar%2Frun%2Fpostgresql/testdb
    // export DATABASE_URL=postgresql://pi@localhost/testdb

    pub(super) fn postgis_postgres_to_geo(
        table: &str,
        bbox: &Option<Extent>,
        srid: i32,
        count: usize,
    ) -> std::result::Result<(), postgres::error::Error> {
        let mut client = Client::connect(&std::env::var("DATABASE_URL").unwrap(), NoTls)?;

        let mut sql = format!("SELECT geom FROM {}", table);
        if let Some(bbox) = bbox {
            sql += &format!(
                " WHERE geom && ST_MakeEnvelope({}, {}, {}, {}, {})",
                bbox.minx, bbox.miny, bbox.maxx, bbox.maxy, srid
            );
        }

        let mut cnt = 0;
        for row in &client.query(sql.as_str(), &[]).unwrap() {
            let _geom: GeoZeroGeometry = row.get(0);
            cnt += 1;
        }
        assert_eq!(cnt, count);
        Ok(())
    }
}

mod rust_postgis {
    use geozero::Extent;
    // use geo::algorithm::from_postgis::FromPostgis;
    use postgis::ewkb;
    use postgres::{self, Client, NoTls};

    // export DATABASE_URL=postgresql://pi@%2Fvar%2Frun%2Fpostgresql/testdb
    // export DATABASE_URL=postgresql://pi@localhost/testdb

    pub(super) fn rust_postgis_read(
        table: &str,
        bbox: &Option<Extent>,
        srid: i32,
        count: usize,
    ) -> std::result::Result<(), postgres::error::Error> {
        let mut client = Client::connect(&std::env::var("DATABASE_URL").unwrap(), NoTls)?;

        let mut sql = format!("SELECT geom FROM {}", table);
        if let Some(bbox) = bbox {
            sql += &format!(
                " WHERE geom && ST_MakeEnvelope({}, {}, {}, {}, {})",
                bbox.minx, bbox.miny, bbox.maxx, bbox.maxy, srid
            );
        }

        let mut cnt = 0;
        for row in &client.query(sql.as_str(), &[]).unwrap() {
            let _pggeom: ewkb::MultiPolygon = row.get(0);
            // let geom = geo_types::MultiPolygon::from_postgis(&pggeom);
            cnt += 1;
        }
        assert_eq!(cnt, count);
        Ok(())
    }
}

mod postgis_sqlx {
    use futures_util::stream::TryStreamExt;
    use geozero::Extent;
    use sqlx::postgres::PgConnection;
    use sqlx::prelude::*;

    // export DATABASE_URL=postgresql://pi@%2Fvar%2Frun%2Fpostgresql/testdb
    // export DATABASE_URL=postgresql://pi@localhost/testdb

    pub(super) async fn postgis_sqlx_to_geo(
        table: &str,
        bbox: &Option<Extent>,
        srid: i32,
        count: usize,
    ) -> std::result::Result<(), sqlx::Error> {
        use geozero_core::postgis::sqlx::geo::Geometry as GeoZeroGeometry;

        let mut conn = PgConnection::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

        let mut sql = format!("SELECT geom FROM {}", table);
        if let Some(bbox) = bbox {
            sql += &format!(
                " WHERE geom && ST_MakeEnvelope({}, {}, {}, {}, {})",
                bbox.minx, bbox.miny, bbox.maxx, bbox.maxy, srid
            );
        }
        let mut cursor = sqlx::query(&sql).fetch(&mut conn);

        let mut cnt = 0;
        while let Some(row) = cursor.try_next().await? {
            let _geom = row.get::<GeoZeroGeometry, _>(0);
            cnt += 1;
        }
        assert_eq!(cnt, count);

        Ok(())
    }
}

mod gpkg {
    use futures_util::stream::TryStreamExt;
    use geozero::Extent;
    use sqlx::prelude::*;
    use sqlx::sqlite::SqliteConnection;

    pub(super) async fn gpkg_to_geo(
        fpath: &str,
        table: &str,
        bbox: &Option<Extent>,
        count: usize,
    ) -> std::result::Result<(), sqlx::Error> {
        use geozero_core::gpkg::geo::Geometry as GeoZeroGeometry;

        let mut conn = SqliteConnection::connect(&format!("sqlite://{}", fpath)).await?;

        // http://erouault.blogspot.com/2017/03/dealing-with-huge-vector-geopackage.html
        let mut sql = format!("SELECT geom FROM {}", table);
        if let Some(bbox) = bbox {
            sql += &format!(
                " JOIN rtree_{}_geom r ON {}.fid = r.id
                    WHERE r.minx <= {} AND r.maxx >= {} AND
                          r.miny <= {} AND r.maxy >= {}",
                table, table, bbox.maxx, bbox.minx, bbox.maxy, bbox.miny
            );
        }
        let mut cursor = sqlx::query(&sql).fetch(&mut conn);
        let mut cnt = 0;
        while let Some(row) = cursor.try_next().await? {
            let _geom = row.get::<GeoZeroGeometry, _>(0);
            cnt += 1;
        }
        assert_eq!(cnt, count);
        Ok(())
    }
}

mod gdal {
    use gdal::vector::{Dataset, Geometry, Layer};
    use geozero::Extent;
    use std::path::Path;

    pub(super) fn gdal_read(
        fpath: &str,
        bbox: &Option<Extent>,
        count: usize,
    ) -> std::result::Result<(), gdal::errors::Error> {
        let mut dataset = Dataset::open(Path::new(fpath))?;
        let layer = dataset.layer(0)?;
        // omit fields when fetching features
        ignore_fields(&layer);

        if let Some(bbox) = bbox {
            let bbox = Geometry::bbox(bbox.minx, bbox.miny, bbox.maxx, bbox.maxy)?;
            layer.set_spatial_filter(&bbox);
        }

        let mut cnt = 0;
        for feature in layer.features() {
            let _geom = feature.geometry();
            cnt += 1;
        }
        assert_eq!(cnt, count);
        Ok(())
    }

    fn ignore_fields(layer: &Layer) {
        let defn = layer.defn();
        let count = unsafe { gdal_sys::OGR_FD_GetFieldCount(defn.c_defn()) };
        for i in 0..count {
            let c_field_defn = unsafe { gdal_sys::OGR_FD_GetFieldDefn(defn.c_defn(), i) };
            unsafe { gdal_sys::OGR_Fld_SetIgnored(c_field_defn, 1) }
        }
    }
}

fn countries_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("countries");
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let bbox = None;
    let srid = 4326;
    group.bench_function("1-shp", |b| {
        b.iter(|| gdal::gdal_read("tests/data/countries.shp", &bbox, 179))
    });
    group.bench_function("2-fgb", |b| {
        b.iter(|| fgb::fgb_to_geo("tests/data/countries.fgb", &bbox, 179))
    });
    group.bench_function("3-gpkg", |b| {
        b.iter(|| {
            rt.block_on(gpkg::gpkg_to_geo(
                "tests/data/countries.gpkg",
                "countries",
                &bbox,
                179,
            ))
        })
    });
    group.bench_function("4-geojson", |b| {
        b.iter(|| gdal::gdal_read("tests/data/countries.json", &bbox, 179))
    });
    group.bench_function("5-geojson_http", |b| {
        b.iter(|| gdal::gdal_read("http://127.0.0.1:3333/countries.json", &bbox, 179))
    });
    group.bench_function("5-geojson_http_gz", |b| {
        b.iter(|| gdal::gdal_read("http://127.0.0.1:3333/countries-gz.json", &bbox, 179))
    });
    group.bench_function("6-fgb_http", |b| {
        b.iter(|| rt.block_on(fgb::fgb_http_to_geo("countries.fgb", &bbox, 179)));
    });
    group.bench_function("7-postgis_sqlx", |b| {
        b.iter(|| {
            rt.block_on(postgis_sqlx::postgis_sqlx_to_geo(
                "countries",
                &bbox,
                srid,
                179,
            ))
        });
    });
    group.bench_function("7-postgis_postgres", |b| {
        b.iter(|| postgis_postgres::postgis_postgres_to_geo("countries", &bbox, srid, 179))
    });
    group.bench_function("7-rust_postgis", |b| {
        b.iter(|| rust_postgis::rust_postgis_read("countries", &bbox, srid, 179))
    });
    group.finish()
}

fn countries_bbox_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("countries_bbox");
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let bbox = Some(Extent {
        minx: 8.8,
        miny: 47.2,
        maxx: 9.5,
        maxy: 55.3,
    });
    let srid = 4326;
    group.bench_function("1-shp", |b| {
        b.iter(|| gdal::gdal_read("tests/data/countries.shp", &bbox, 3))
        // != 6 ! within filter?
    });
    group.bench_function("2-fgb", |b| {
        b.iter(|| fgb::fgb_to_geo("tests/data/countries.fgb", &bbox, 6))
    });
    group.bench_function("3-gpkg", |b| {
        b.iter(|| {
            rt.block_on(gpkg::gpkg_to_geo(
                "tests/data/countries.gpkg",
                "countries",
                &bbox,
                6,
            ))
        })
    });
    group.bench_function("3-gpkg_gdal", |b| {
        b.iter(|| {
            gdal::gdal_read(
                "tests/data/countries.gpkg",
                &bbox,
                3, // != 6!
            )
        })
    });
    group.bench_function("4-geojson", |b| {
        b.iter(|| gdal::gdal_read("tests/data/countries.json", &bbox, 3))
    });
    group.bench_function("5-geojson_http", |b| {
        b.iter(|| gdal::gdal_read("http://127.0.0.1:3333/countries.json", &bbox, 3))
    });
    group.bench_function("5-geojson_http_gz", |b| {
        b.iter(|| gdal::gdal_read("http://127.0.0.1:3333/countries-gz.json", &bbox, 3))
    });
    group.bench_function("6-fgb_http", |b| {
        b.iter(|| rt.block_on(fgb::fgb_http_to_geo("countries.fgb", &bbox, 6)));
    });
    group.bench_function("7-postgis_sqlx", |b| {
        b.iter(|| {
            rt.block_on(postgis_sqlx::postgis_sqlx_to_geo(
                "countries",
                &bbox,
                srid,
                6,
            ))
        })
    });
    group.bench_function("7-postgis_postgres", |b| {
        b.iter(|| postgis_postgres::postgis_postgres_to_geo("countries", &bbox, srid, 6))
    });
    group.bench_function("7-rust_postgis", |b| {
        b.iter(|| rust_postgis::rust_postgis_read("countries", &bbox, srid, 6))
    });
    group.finish()
}

fn buildings_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("buildings");
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let bbox = None;
    let srid = 3857;
    group.bench_function("2-fgb", |b| {
        b.iter(|| fgb::fgb_to_geo("tests/data/osm-buildings-3857-ch.fgb", &bbox, 2407771))
    });
    group.bench_function("1-shp", |b| {
        b.iter(|| gdal::gdal_read("tests/data/osm-buildings-3857-ch.shp", &bbox, 2407771))
    });
    if std::env::var("SKIP_GPKG_BIG").is_err() {
        // A test machine freezes when running this bench !!??
        group.bench_function("3-gpkg", |b| {
            b.iter(|| {
                rt.block_on(gpkg::gpkg_to_geo(
                    "tests/data/osm-buildings-3857-ch.gpkg",
                    "buildings",
                    &bbox,
                    2407771,
                ))
            })
        });
    }
    // > 50s
    // group.bench_function("4-geojson_gz", |b| {
    //     b.iter(|| {
    //         gdal::gdal_read(
    //             "/vsigzip/tests/data/osm-buildings-3857-ch-gz.json.gz",
    //             &bbox,
    //             2407771,
    //         )
    //     })
    // });
    // > 60s
    // group.bench_function("5-geojson_http", |b| {
    //     b.iter(|| {
    //         gdal::gdal_read(
    //             "http://127.0.0.1:3333/osm-buildings-3857-ch.json",
    //             &bbox,
    //             2407771,
    //         )
    //     })
    // });
    group.bench_function("6-fgb_http", |b| {
        b.iter(|| {
            rt.block_on(fgb::fgb_http_to_geo(
                "osm-buildings-3857-ch.fgb",
                &bbox,
                2407771,
            ))
        });
    });
    // group.bench_function("7-postgis_sqlx", |b| {
    //     b.iter(|| {
    //         rt.block_on(postgis_sqlx::postgis_sqlx_to_geo(
    //             "buildings",
    //             &bbox,
    //             srid,
    //             2407771,
    //         ))
    //     })
    // });
    group.bench_function("7-postgis_postgres", |b| {
        b.iter(|| postgis_postgres::postgis_postgres_to_geo("buildings", &bbox, srid, 2407771))
    });
    // group.bench_function("7-rust_postgis", |b| {
    //     b.iter(|| rust_postgis::rust_postgis_read("buildings", &bbox, srid, 2407771))
    // });
    group.finish()
}

fn buildings_bbox_benchmark(c: &mut Criterion) {
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("buildings_bbox");
    let bbox = Some(Extent {
        minx: 939651.0,
        miny: 5997817.0,
        maxx: 957733.0,
        maxy: 6012256.0,
    });
    let srid = 3857;
    group.bench_function("1-shp", |b| {
        b.iter(|| gdal::gdal_read("tests/data/osm-buildings-3857-ch.shp", &bbox, 54351))
    });
    group.bench_function("2-fgb", |b| {
        b.iter(|| fgb::fgb_to_geo("tests/data/osm-buildings-3857-ch.fgb", &bbox, 54351))
    });
    group.bench_function("3-gpkg", |b| {
        b.iter(|| {
            rt.block_on(gpkg::gpkg_to_geo(
                "tests/data/osm-buildings-3857-ch.gpkg",
                "buildings",
                &bbox,
                54355, // fgb: 54351
            ))
        })
    });
    group.bench_function("3-gpkg_gdal", |b| {
        b.iter(|| gdal::gdal_read("tests/data/osm-buildings-3857-ch.gpkg", &bbox, 54351))
    });
    group.bench_function("6-fgb_http", |b| {
        b.iter(|| {
            rt.block_on(fgb::fgb_http_to_geo(
                "osm-buildings-3857-ch.fgb",
                &bbox,
                54351,
            ))
        });
    });
    group.bench_function("7-postgis_sqlx", |b| {
        b.iter(|| {
            rt.block_on(postgis_sqlx::postgis_sqlx_to_geo(
                "buildings",
                &bbox,
                srid,
                54353, // fgb: 54351
            ))
        })
    });
    group.bench_function("7-postgis_postgres", |b| {
        b.iter(|| {
            postgis_postgres::postgis_postgres_to_geo(
                "buildings",
                &bbox,
                srid,
                54353, // fgb: 54351
            )
        })
    });
    group.bench_function("7-rust_postgis", |b| {
        b.iter(|| rust_postgis::rust_postgis_read("buildings", &bbox, srid, 54353))
        // fgb: 54351
    });
    group.finish()
}

criterion_group!(name=benches; config=Criterion::default().sample_size(10);
                 targets=countries_benchmark,countries_bbox_benchmark,buildings_bbox_benchmark,buildings_benchmark);
criterion_main!(benches);
