use super::{
   imsa_widths::ImsaColumnWidths,
   series_widths::{
      F1ColumnWidths,
      NlsColumnWidths,
   },
   wec_widths::WecColumnWidths,
};

#[derive(Clone, Copy, Default)]
pub struct TableWidthBaselines<'a> {
   pub imsa: Option<&'a ImsaColumnWidths>,
   pub nls:  Option<&'a NlsColumnWidths>,
   pub f1:   Option<&'a F1ColumnWidths>,
   pub wec:  Option<&'a WecColumnWidths>,
}
