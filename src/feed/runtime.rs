use crate::timing::Series;

pub fn source_id_for(series: Series) -> u64 {
    match series {
        Series::Dhlm => 5,
        Series::Imsa => 1,
        Series::Nls => 2,
        Series::F1 => 3,
        Series::Wec => 4,
    }
}
