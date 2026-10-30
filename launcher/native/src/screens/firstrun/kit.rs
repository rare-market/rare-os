//! Shared pieces for the first-run steps (launcher/src/onboarding/kit.tsx): a step body with a title,
//! scrolling content and a button row, a done line, a working bar and a choice row.

use crate::screens::wallet::markup::*;

/// `Step`: title block on top, the step's content (scrolls on its own), its buttons.
pub fn step<M>(title: El<M>, body: Option<El<M>>, content: Vec<El<M>>, actions: Vec<El<M>>, wide: bool) -> El<M> {
    let mut head = div("shrink-0");
    let mut h = h1("text-[2.25rem] font-semibold leading-[1.1] tracking-[-0.03em]");
    if title.tag == Tag::Span && title.class.is_empty() && title.kids.is_empty() {
        h.text = title.text;
    } else {
        h = h.kid(title);
    }
    head = head.kid(h);
    if let Some(b) = body {
        let mut pp = p("mt-[0.75rem] max-w-[44rem] text-body leading-relaxed text-muted");
        if b.tag == Tag::Span && b.class.is_empty() && b.kids.is_empty() {
            pp.text = b.text;
        } else {
            pp = pp.kid(b);
        }
        head = head.kid(pp);
    }
    let mut d = div(cn(&["mx-auto flex h-full w-full flex-col pb-[1.25rem] pt-[1.5rem]", if wide { "max-w-[66rem]" } else { "max-w-[48rem]" }]))
        .kid(head)
        .kid(div("-mx-[0.75rem] mt-[1.1rem] min-h-0 flex-1 overflow-y-auto px-[0.75rem] py-[0.5rem]").attr("data-scroll", "").kids(content));
    if !actions.is_empty() {
        d = d.kid(div("mt-[1rem] flex shrink-0 items-center gap-[0.75rem]").attr("data-nav-group", "step-actions").attr("data-nav-remember", "").kids(actions));
    }
    d
}

/// `ChoiceRow`: a focusable radio row; `on` None draws no radio.
pub fn choice_row<M>(on: Option<bool>, icon_el: Option<El<M>>, title: &str, body: Option<&str>, def: bool, msg: M) -> El<M> {
    let mut b = button(cn(&["flex w-full items-center gap-[1rem] rounded-card border px-[1.25rem] py-[1rem] text-left", if on == Some(true) { "border-line-2 bg-l2" } else { "border-hair bg-l1" }]), Some(msg))
        .def(def)
        .role("radio")
        .attr("aria-checked", (on == Some(true)).to_string());
    if let Some(i) = icon_el {
        b = b.kid(span("flex h-[2.75rem] w-[2.75rem] shrink-0 items-center justify-center rounded-full bg-l3 text-fg").kid(i));
    }
    b = b.kid(span("min-w-0 flex-1").kid(span("block text-body font-semibold").text(title)).kid_opt(body.map(|x| span("mt-[0.15rem] block text-small text-muted").text(x))));
    if let Some(o) = on {
        b = b.kid(span(cn(&["flex h-[1.25rem] w-[1.25rem] shrink-0 items-center justify-center rounded-full border", if o { "border-fg" } else { "border-line-2" }])).kid_if(o, || span("h-[0.6rem] w-[0.6rem] rounded-full bg-fg")));
    }
    b
}

/// `DoneLine`: something finished, the one mark these steps use.
pub fn done_line<M>(kids: Vec<El<M>>) -> El<M> {
    div("flex items-center gap-[0.7rem] text-body").kid(span("flex h-[1.6rem] w-[1.6rem] shrink-0 items-center justify-center rounded-full bg-l3 text-fg").kid(icon("check", "").attr("size", "1rem"))).kid(span("").kids(kids))
}

/// `Working`: a thin indeterminate bar with a line of text.
pub fn working<M>(on: bool, label: Option<&str>) -> El<M> {
    div("flex flex-col gap-[0.6rem]")
        .kid(div("relative h-[2px] w-full overflow-hidden rounded-full bg-fg/10").kid_if(on, || div("load-bar absolute inset-0")))
        .kid_opt(label.map(|l| div("text-small text-muted").text(l)))
}
