//! Application types and `SchemaTypes` bindings for the large-consumer fixture.

use crate::enum_bindings::bind_enum;
use crate::generated;

/// Synthetic domain object returned by every object resolver in this fixture.
pub struct Product;

/// 5 is a synthetic domain scalar commonly used for custom scalars.
pub struct ScalarValue;

/// Synthetic domain enum commonly used for binding object 65 enum.
#[derive(Clone, Copy)]
pub enum DomainKind {
    Alpha,
    Beta,
}

impl chigiri::Scalar for ScalarValue {
    fn parse(_value: chigiri::Value) -> Result<Self, chigiri::ScalarCoercionError> {
        Ok(Self)
    }

    fn serialize(&self) -> chigiri::FieldResult<chigiri::Value> {
        Ok(chigiri::Value::String("synthetic".to_owned()))
    }
}

/// Application type mappings for the large schema fixture.
pub struct Types;

impl generated::SchemaTypes for Types {
    bind_enum!(Enum000, enum000_from_graphql, enum000_to_graphql);
    bind_enum!(Enum001, enum001_from_graphql, enum001_to_graphql);
    bind_enum!(Enum002, enum002_from_graphql, enum002_to_graphql);
    bind_enum!(Enum003, enum003_from_graphql, enum003_to_graphql);
    bind_enum!(Enum004, enum004_from_graphql, enum004_to_graphql);
    bind_enum!(Enum005, enum005_from_graphql, enum005_to_graphql);
    bind_enum!(Enum006, enum006_from_graphql, enum006_to_graphql);
    bind_enum!(Enum007, enum007_from_graphql, enum007_to_graphql);
    bind_enum!(Enum008, enum008_from_graphql, enum008_to_graphql);
    bind_enum!(Enum009, enum009_from_graphql, enum009_to_graphql);
    bind_enum!(Enum010, enum010_from_graphql, enum010_to_graphql);
    bind_enum!(Enum011, enum011_from_graphql, enum011_to_graphql);
    bind_enum!(Enum012, enum012_from_graphql, enum012_to_graphql);
    bind_enum!(Enum013, enum013_from_graphql, enum013_to_graphql);
    bind_enum!(Enum014, enum014_from_graphql, enum014_to_graphql);
    bind_enum!(Enum015, enum015_from_graphql, enum015_to_graphql);
    bind_enum!(Enum016, enum016_from_graphql, enum016_to_graphql);
    bind_enum!(Enum017, enum017_from_graphql, enum017_to_graphql);
    bind_enum!(Enum018, enum018_from_graphql, enum018_to_graphql);
    bind_enum!(Enum019, enum019_from_graphql, enum019_to_graphql);
    bind_enum!(Enum020, enum020_from_graphql, enum020_to_graphql);
    bind_enum!(Enum021, enum021_from_graphql, enum021_to_graphql);
    bind_enum!(Enum022, enum022_from_graphql, enum022_to_graphql);
    bind_enum!(Enum023, enum023_from_graphql, enum023_to_graphql);
    bind_enum!(Enum024, enum024_from_graphql, enum024_to_graphql);
    bind_enum!(Enum025, enum025_from_graphql, enum025_to_graphql);
    bind_enum!(Enum026, enum026_from_graphql, enum026_to_graphql);
    bind_enum!(Enum027, enum027_from_graphql, enum027_to_graphql);
    bind_enum!(Enum028, enum028_from_graphql, enum028_to_graphql);
    bind_enum!(Enum029, enum029_from_graphql, enum029_to_graphql);
    bind_enum!(Enum030, enum030_from_graphql, enum030_to_graphql);
    bind_enum!(Enum031, enum031_from_graphql, enum031_to_graphql);
    bind_enum!(Enum032, enum032_from_graphql, enum032_to_graphql);
    bind_enum!(Enum033, enum033_from_graphql, enum033_to_graphql);
    bind_enum!(Enum034, enum034_from_graphql, enum034_to_graphql);
    bind_enum!(Enum035, enum035_from_graphql, enum035_to_graphql);
    bind_enum!(Enum036, enum036_from_graphql, enum036_to_graphql);
    bind_enum!(Enum037, enum037_from_graphql, enum037_to_graphql);
    bind_enum!(Enum038, enum038_from_graphql, enum038_to_graphql);
    bind_enum!(Enum039, enum039_from_graphql, enum039_to_graphql);
    bind_enum!(Enum040, enum040_from_graphql, enum040_to_graphql);
    bind_enum!(Enum041, enum041_from_graphql, enum041_to_graphql);
    bind_enum!(Enum042, enum042_from_graphql, enum042_to_graphql);
    bind_enum!(Enum043, enum043_from_graphql, enum043_to_graphql);
    bind_enum!(Enum044, enum044_from_graphql, enum044_to_graphql);
    bind_enum!(Enum045, enum045_from_graphql, enum045_to_graphql);
    bind_enum!(Enum046, enum046_from_graphql, enum046_to_graphql);
    bind_enum!(Enum047, enum047_from_graphql, enum047_to_graphql);
    bind_enum!(Enum048, enum048_from_graphql, enum048_to_graphql);
    bind_enum!(Enum049, enum049_from_graphql, enum049_to_graphql);
    bind_enum!(Enum050, enum050_from_graphql, enum050_to_graphql);
    bind_enum!(Enum051, enum051_from_graphql, enum051_to_graphql);
    bind_enum!(Enum052, enum052_from_graphql, enum052_to_graphql);
    bind_enum!(Enum053, enum053_from_graphql, enum053_to_graphql);
    bind_enum!(Enum054, enum054_from_graphql, enum054_to_graphql);
    bind_enum!(Enum055, enum055_from_graphql, enum055_to_graphql);
    bind_enum!(Enum056, enum056_from_graphql, enum056_to_graphql);
    bind_enum!(Enum057, enum057_from_graphql, enum057_to_graphql);
    bind_enum!(Enum058, enum058_from_graphql, enum058_to_graphql);
    bind_enum!(Enum059, enum059_from_graphql, enum059_to_graphql);
    bind_enum!(Enum060, enum060_from_graphql, enum060_to_graphql);
    bind_enum!(Enum061, enum061_from_graphql, enum061_to_graphql);
    bind_enum!(Enum062, enum062_from_graphql, enum062_to_graphql);
    bind_enum!(Enum063, enum063_from_graphql, enum063_to_graphql);
    bind_enum!(Enum064, enum064_from_graphql, enum064_to_graphql);
    type Scalar000 = ScalarValue;
    type Scalar001 = ScalarValue;
    type Scalar002 = ScalarValue;
    type Scalar003 = ScalarValue;
    type Scalar004 = ScalarValue;
    type ProductObject000 = Product;
    type ProductObject001 = Product;
    type ProductObject002 = Product;
    type ProductObject003 = Product;
    type ProductObject004 = Product;
    type ProductObject005 = Product;
    type ProductObject006 = Product;
    type ProductObject007 = Product;
    type ProductObject008 = Product;
    type ProductObject009 = Product;
    type ProductObject010 = Product;
    type ProductObject011 = Product;
    type ProductObject012 = Product;
    type ProductObject013 = Product;
    type ProductObject014 = Product;
    type ProductObject015 = Product;
    type ProductObject016 = Product;
    type ProductObject017 = Product;
    type ProductObject018 = Product;
    type ProductObject019 = Product;
    type ProductObject020 = Product;
    type ProductObject021 = Product;
    type ProductObject022 = Product;
    type ProductObject023 = Product;
    type ProductObject024 = Product;
    type ProductObject025 = Product;
    type ProductObject026 = Product;
    type ProductObject027 = Product;
    type ProductObject028 = Product;
    type ProductObject029 = Product;
    type ProductObject030 = Product;
    type ProductObject031 = Product;
    type ProductObject032 = Product;
    type ProductObject033 = Product;
    type ProductObject034 = Product;
    type ProductObject035 = Product;
    type ProductObject036 = Product;
    type ProductObject037 = Product;
    type ProductObject038 = Product;
    type ProductObject039 = Product;
    type ProductObject040 = Product;
    type ProductObject041 = Product;
    type ProductObject042 = Product;
    type ProductObject043 = Product;
    type ProductObject044 = Product;
    type ProductObject045 = Product;
    type ProductObject046 = Product;
    type ProductObject047 = Product;
    type ProductObject048 = Product;
    type ProductObject049 = Product;
    type ProductObject050 = Product;
    type ProductObject051 = Product;
    type ProductObject052 = Product;
    type ProductObject053 = Product;
    type ProductObject054 = Product;
    type ProductObject055 = Product;
    type ProductObject056 = Product;
    type ProductObject057 = Product;
    type ProductObject058 = Product;
    type ProductObject059 = Product;
    type ProductObject060 = Product;
    type ProductObject061 = Product;
    type ProductObject062 = Product;
    type ProductObject063 = Product;
    type ProductObject064 = Product;
    type ProductObject065 = Product;
    type ProductObject066 = Product;
    type ProductObject067 = Product;
    type ProductObject068 = Product;
}
