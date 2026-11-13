//! Connects every synthetic schema type and all 65 enum bindings to local types.

use crate::bindings::{external_enum, identity_enum};
use crate::{SampleMeasurement, ScalarValue};

/// Application-owned type connections for the complete fixture schema.
pub struct Types;

impl contract::SchemaTypes for Types {
    type FixtureScalar000 = ScalarValue;
    type FixtureScalar001 = ScalarValue;
    type FixtureRecord000 = ();
    type FixtureScalar002 = ScalarValue;
    type FixtureScalar003 = ScalarValue;
    type FixtureScalar004 = ScalarValue;
    identity_enum!(
        FixtureChoice000,
        fixture_choice000_from_graphql,
        fixture_choice000_to_graphql
    );
    identity_enum!(
        FixtureChoice001,
        fixture_choice001_from_graphql,
        fixture_choice001_to_graphql
    );
    identity_enum!(
        FixtureChoice002,
        fixture_choice002_from_graphql,
        fixture_choice002_to_graphql
    );
    external_enum!(
        UnitKind,
        UnitKind,
        unit_kind_from_graphql,
        unit_kind_to_graphql,
        [Value173, Value075, Value174]
    );
    identity_enum!(
        FixtureChoice004,
        fixture_choice004_from_graphql,
        fixture_choice004_to_graphql
    );
    identity_enum!(
        FixtureChoice005,
        fixture_choice005_from_graphql,
        fixture_choice005_to_graphql
    );
    identity_enum!(
        FixtureChoice006,
        fixture_choice006_from_graphql,
        fixture_choice006_to_graphql
    );
    identity_enum!(
        FixtureChoice007,
        fixture_choice007_from_graphql,
        fixture_choice007_to_graphql
    );
    identity_enum!(
        FixtureChoice008,
        fixture_choice008_from_graphql,
        fixture_choice008_to_graphql
    );
    identity_enum!(
        FixtureChoice009,
        fixture_choice009_from_graphql,
        fixture_choice009_to_graphql
    );
    identity_enum!(
        FixtureChoice010,
        fixture_choice010_from_graphql,
        fixture_choice010_to_graphql
    );
    identity_enum!(
        FixtureChoice011,
        fixture_choice011_from_graphql,
        fixture_choice011_to_graphql
    );
    identity_enum!(
        FixtureChoice012,
        fixture_choice012_from_graphql,
        fixture_choice012_to_graphql
    );
    identity_enum!(
        FixtureChoice013,
        fixture_choice013_from_graphql,
        fixture_choice013_to_graphql
    );
    identity_enum!(
        FixtureChoice014,
        fixture_choice014_from_graphql,
        fixture_choice014_to_graphql
    );
    identity_enum!(
        FixtureChoice015,
        fixture_choice015_from_graphql,
        fixture_choice015_to_graphql
    );
    identity_enum!(
        FixtureChoice016,
        fixture_choice016_from_graphql,
        fixture_choice016_to_graphql
    );
    identity_enum!(
        FixtureChoice017,
        fixture_choice017_from_graphql,
        fixture_choice017_to_graphql
    );
    identity_enum!(
        FixtureChoice018,
        fixture_choice018_from_graphql,
        fixture_choice018_to_graphql
    );
    identity_enum!(
        FixtureChoice019,
        fixture_choice019_from_graphql,
        fixture_choice019_to_graphql
    );
    identity_enum!(
        FixtureChoice020,
        fixture_choice020_from_graphql,
        fixture_choice020_to_graphql
    );
    identity_enum!(
        FixtureChoice021,
        fixture_choice021_from_graphql,
        fixture_choice021_to_graphql
    );
    identity_enum!(
        FixtureChoice022,
        fixture_choice022_from_graphql,
        fixture_choice022_to_graphql
    );
    identity_enum!(
        FixtureChoice023,
        fixture_choice023_from_graphql,
        fixture_choice023_to_graphql
    );
    identity_enum!(
        FixtureChoice024,
        fixture_choice024_from_graphql,
        fixture_choice024_to_graphql
    );
    identity_enum!(
        FixtureChoice025,
        fixture_choice025_from_graphql,
        fixture_choice025_to_graphql
    );
    identity_enum!(
        FixtureChoice026,
        fixture_choice026_from_graphql,
        fixture_choice026_to_graphql
    );
    identity_enum!(
        FixtureChoice027,
        fixture_choice027_from_graphql,
        fixture_choice027_to_graphql
    );
    external_enum!(
        FormatKind,
        FormatKind,
        format_kind_from_graphql,
        format_kind_to_graphql,
        [Value176, Value180, Value179]
    );
    identity_enum!(
        FixtureChoice029,
        fixture_choice029_from_graphql,
        fixture_choice029_to_graphql
    );
    identity_enum!(
        FixtureChoice030,
        fixture_choice030_from_graphql,
        fixture_choice030_to_graphql
    );
    identity_enum!(
        FixtureChoice031,
        fixture_choice031_from_graphql,
        fixture_choice031_to_graphql
    );
    identity_enum!(
        FixtureChoice032,
        fixture_choice032_from_graphql,
        fixture_choice032_to_graphql
    );
    identity_enum!(
        FixtureChoice033,
        fixture_choice033_from_graphql,
        fixture_choice033_to_graphql
    );
    identity_enum!(
        FixtureChoice034,
        fixture_choice034_from_graphql,
        fixture_choice034_to_graphql
    );
    identity_enum!(
        FixtureChoice035,
        fixture_choice035_from_graphql,
        fixture_choice035_to_graphql
    );
    identity_enum!(
        FixtureChoice036,
        fixture_choice036_from_graphql,
        fixture_choice036_to_graphql
    );
    external_enum!(
        TransportKind,
        ExternalTransport,
        transport_kind_from_graphql,
        transport_kind_to_graphql,
        [Value168, Value169]
    );
    identity_enum!(
        FixtureChoice038,
        fixture_choice038_from_graphql,
        fixture_choice038_to_graphql
    );
    identity_enum!(
        FixtureChoice039,
        fixture_choice039_from_graphql,
        fixture_choice039_to_graphql
    );
    identity_enum!(
        FixtureChoice040,
        fixture_choice040_from_graphql,
        fixture_choice040_to_graphql
    );
    identity_enum!(
        FixtureChoice041,
        fixture_choice041_from_graphql,
        fixture_choice041_to_graphql
    );
    identity_enum!(
        FixtureChoice042,
        fixture_choice042_from_graphql,
        fixture_choice042_to_graphql
    );
    identity_enum!(
        FixtureChoice043,
        fixture_choice043_from_graphql,
        fixture_choice043_to_graphql
    );
    identity_enum!(
        FixtureChoice044,
        fixture_choice044_from_graphql,
        fixture_choice044_to_graphql
    );
    identity_enum!(
        FixtureChoice045,
        fixture_choice045_from_graphql,
        fixture_choice045_to_graphql
    );
    identity_enum!(
        FixtureChoice046,
        fixture_choice046_from_graphql,
        fixture_choice046_to_graphql
    );
    identity_enum!(
        FixtureChoice047,
        fixture_choice047_from_graphql,
        fixture_choice047_to_graphql
    );
    identity_enum!(
        FixtureChoice048,
        fixture_choice048_from_graphql,
        fixture_choice048_to_graphql
    );
    identity_enum!(
        FixtureChoice049,
        fixture_choice049_from_graphql,
        fixture_choice049_to_graphql
    );
    identity_enum!(
        FixtureChoice050,
        fixture_choice050_from_graphql,
        fixture_choice050_to_graphql
    );
    identity_enum!(
        FixtureChoice051,
        fixture_choice051_from_graphql,
        fixture_choice051_to_graphql
    );
    identity_enum!(
        FixtureChoice052,
        fixture_choice052_from_graphql,
        fixture_choice052_to_graphql
    );
    identity_enum!(
        FixtureChoice053,
        fixture_choice053_from_graphql,
        fixture_choice053_to_graphql
    );
    identity_enum!(
        FixtureChoice054,
        fixture_choice054_from_graphql,
        fixture_choice054_to_graphql
    );
    identity_enum!(
        FixtureChoice055,
        fixture_choice055_from_graphql,
        fixture_choice055_to_graphql
    );
    identity_enum!(
        FixtureChoice056,
        fixture_choice056_from_graphql,
        fixture_choice056_to_graphql
    );
    identity_enum!(
        FixtureChoice057,
        fixture_choice057_from_graphql,
        fixture_choice057_to_graphql
    );
    identity_enum!(
        FixtureChoice058,
        fixture_choice058_from_graphql,
        fixture_choice058_to_graphql
    );
    identity_enum!(
        FixtureChoice059,
        fixture_choice059_from_graphql,
        fixture_choice059_to_graphql
    );
    identity_enum!(
        FixtureChoice060,
        fixture_choice060_from_graphql,
        fixture_choice060_to_graphql
    );
    identity_enum!(
        FixtureChoice061,
        fixture_choice061_from_graphql,
        fixture_choice061_to_graphql
    );
    identity_enum!(
        FixtureChoice062,
        fixture_choice062_from_graphql,
        fixture_choice062_to_graphql
    );
    identity_enum!(
        FixtureChoice063,
        fixture_choice063_from_graphql,
        fixture_choice063_to_graphql
    );
    identity_enum!(
        FixtureChoice064,
        fixture_choice064_from_graphql,
        fixture_choice064_to_graphql
    );
    type FixtureRecord001 = ();
    type FixtureRecord002 = ();
    type FixtureRecord003 = ();
    type FixtureRecord004 = ();
    type FixtureRecord005 = ();
    type FixtureRecord006 = ();
    type FixtureRecord007 = ();
    type FixtureRecord008 = ();
    type FixtureRecord009 = ();
    type FixtureRecord010 = ();
    type FixtureRecord011 = ();
    type FixtureRecord012 = ();
    type FixtureRecord013 = ();
    type FixtureRecord014 = ();
    type FixtureRecord015 = ();
    type FixtureRecord016 = ();
    type FixtureRecord017 = ();
    type FixtureRecord018 = ();
    type FixtureRecord019 = ();
    type FixtureRecord020 = ();
    type FixtureRecord021 = ();
    type FixtureRecord022 = ();
    type FixtureRecord023 = ();
    type FixtureRecord024 = ();
    type FixtureRecord025 = ();
    type FixtureRecord026 = ();
    type FixtureRecord027 = ();
    type FixtureRecord028 = ();
    type FixtureRecord029 = ();
    type FixtureRecord030 = ();
    type FixtureRecord031 = ();
    type FixtureRecord032 = ();
    type FixtureRecord033 = ();
    type FixtureRecord034 = ();
    type FixtureRecord035 = ();
    type FixtureRecord036 = ();
    type FixtureRecord037 = ();
    type FixtureRecord038 = ();
    type FixtureRecord039 = ();
    type FixtureRecord040 = ();
    type FixtureRecord041 = ();
    type FixtureRecord042 = ();
    type FixtureRecord043 = ();
    type FixtureRecord044 = ();
    type FixtureRecord045 = ();
    type FixtureRecord046 = ();
    type FixtureRecord047 = ();
    type FixtureRecord048 = ();
    type FixtureRecord049 = ();
    type FixtureRecord050 = ();
    type FixtureRecord051 = ();
    type FixtureRecord052 = ();
    type Measurement = SampleMeasurement;
    type FixtureRecord054 = ();
    type FixtureRecord055 = ();
    type FixtureRecord056 = ();
    type FixtureRecord057 = ();
    type FixtureRecord058 = ();
    type FixtureRecord059 = ();
    type FixtureRecord060 = ();
    type FixtureRecord061 = ();
    type FixtureRecord062 = ();
    type FixtureRecord063 = ();
    type FixtureRecord064 = ();
}
