pub mod dem;
mod gdal_pool;
mod instance;
pub mod raster;
mod resample;
mod spatial_ref;

use gdal_pool::GdalPool;
use instance::Instance;
use resample::ResampleAlg;
use spatial_ref::get_spatial_ref;
