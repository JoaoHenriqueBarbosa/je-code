//! Porte de `src/sbcsgroupprober.js`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::charsetgroupprober::{CharsetGroupProber, SharedProber, share};
use crate::hebrewprober::HebrewProber;
use crate::langbulgarianmodel::{LATIN5_BULGARIAN_MODEL, WIN1251_BULGARIAN_MODEL};
use crate::langcyrillicmodel::{IBM855_MODEL, IBM866_MODEL, KOI8R_MODEL, LATIN5_CYRILLIC_MODEL, MAC_CYRILLIC_MODEL, WIN1251_CYRILLIC_MODEL};
use crate::langgreekmodel::{LATIN7_GREEK_MODEL, WIN1253_GREEK_MODEL};
use crate::langhebrewmodel::WIN1255_HEBREW_MODEL;
use crate::langhungarianmodel::{LATIN2_HUNGARIAN_MODEL, WIN1250_HUNGARIAN_MODEL};
use crate::langthaimodel::TIS620_THAI_MODEL;
use crate::sbcharsetprober::{SbModel, SingleByteCharSetProber};

pub fn sbcs_group_prober() -> CharsetGroupProber {
    let models: [&'static SbModel; 13] = [
        &WIN1251_CYRILLIC_MODEL,
        &KOI8R_MODEL,
        &LATIN5_CYRILLIC_MODEL,
        &MAC_CYRILLIC_MODEL,
        &IBM866_MODEL,
        &IBM855_MODEL,
        &LATIN7_GREEK_MODEL,
        &WIN1253_GREEK_MODEL,
        &LATIN5_BULGARIAN_MODEL,
        &WIN1251_BULGARIAN_MODEL,
        &LATIN2_HUNGARIAN_MODEL,
        &WIN1250_HUNGARIAN_MODEL,
        &TIS620_THAI_MODEL,
    ];
    let mut probers: Vec<SharedProber> = models.into_iter().map(|model| share(SingleByteCharSetProber::new(model, false, None))).collect();

    let hebrew_prober = Rc::new(RefCell::new(HebrewProber::new()));
    let logical_hebrew_prober = Rc::new(RefCell::new(SingleByteCharSetProber::new(&WIN1255_HEBREW_MODEL, false, Some(hebrew_prober.clone()))));
    let visual_hebrew_prober = Rc::new(RefCell::new(SingleByteCharSetProber::new(&WIN1255_HEBREW_MODEL, true, Some(hebrew_prober.clone()))));
    hebrew_prober.borrow_mut().set_model_probers(Rc::downgrade(&logical_hebrew_prober), Rc::downgrade(&visual_hebrew_prober));
    probers.push(hebrew_prober);
    probers.push(logical_hebrew_prober);
    probers.push(visual_hebrew_prober);

    CharsetGroupProber::new(probers)
}
