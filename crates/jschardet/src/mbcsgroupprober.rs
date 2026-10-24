//! Porte de `src/mbcsgroupprober.js`.

use crate::big5prober::big5_prober;
use crate::charsetgroupprober::{CharsetGroupProber, share};
use crate::eucjpprober::eucjp_prober;
use crate::euckrprober::euckr_prober;
use crate::euctwprober::euctw_prober;
use crate::gb2312prober::gb2312_prober;
use crate::sjisprober::sjis_prober;
use crate::utf8prober::UTF8Prober;

pub fn mbcs_group_prober() -> CharsetGroupProber {
    CharsetGroupProber::new(vec![
        share(UTF8Prober::new()),
        share(sjis_prober()),
        share(eucjp_prober()),
        share(gb2312_prober()),
        share(euckr_prober()),
        share(big5_prober()),
        share(euctw_prober()),
    ])
}
