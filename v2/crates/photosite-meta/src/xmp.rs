//! The XMP packet: reading what a photograph already says, and putting ours
//! back without losing the rest.
//!
//! **Nothing here replaces a packet wholesale.** A photograph that already
//! carries somebody else's properties — Photoshop's, the camera's, another
//! cataloguer's — keeps every one of them. Ours are taken out of wherever
//! they were and written back in one block of our own; everything else is
//! copied through untouched. Replacing the packet would be the same mistake
//! as handing a metadata writer an empty set and calling it the file's
//! metadata, which is how five kilobytes of EXIF becomes forty-eight bytes.
//!
//! Properties are matched by **namespace URI and never by prefix**. The same
//! namespace is written `xmp:` by most tools and `xap:` by older ones —
//! including, as it happens, by v1 — and a reader that matches on the prefix
//! silently misses half the library.

use photosite_core::domain::{ColorLabel, Organisation};
use photosite_core::place::Place;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{NsReader, Writer};
use std::io::Cursor;

/// `http://ns.adobe.com/xap/1.0/` — where the rating and the label live.
const NS_XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/";
/// `http://purl.org/dc/elements/1.1/` — where the words live.
const NS_DC: &[u8] = b"http://purl.org/dc/elements/1.1/";
const NS_RDF: &[u8] = b"http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// Where a position lives in XMP, whatever the file it sits in.
const NS_EXIF: &[u8] = b"http://ns.adobe.com/exif/1.0/";
/// The Metadata Working Group's face regions — what Lightroom, digiKam and
/// Windows all read a face frame from. Named `mwg-rs:` by everybody, and the
/// URI is what we match on, as everywhere else here.
const NS_MWG_RS: &[u8] = b"http://www.metadataworkinggroup.com/schemas/regions/";

/// What we read out of a packet, and what we put back into one.
// Not `Eq`: a position is two floating-point numbers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Xmp {
    pub rating: Option<u8>,
    pub label: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    /// Where it was taken, when somebody has said so. A position is never
    /// **removed** from a file by us: what we do not know we leave alone,
    /// and a photograph that arrived with coordinates keeps them.
    pub place: Option<Place>,
    /// The named faces, when we have actually looked.
    ///
    /// `None` and an empty list mean two different things, and the
    /// difference is the whole of the rule. `None` is "this photograph has
    /// never been face-scanned", and then whatever frames Lightroom or
    /// Picasa left in it are none of our business. `Some(empty)` is "we
    /// looked and nobody here is named", which is a thing to write — it is
    /// how a face taken off somebody comes back out of the file.
    pub regions: Option<Regions>,
    /// The two halves as they were read, held between reading the latitude
    /// and the longitude. They are separate properties and either can come
    /// first.
    latitude: Option<String>,
    longitude: Option<String>,
}

/// The face frames of one photograph, with the frame they were measured in.
///
/// The pixel dimensions are not decoration: an MWG area is a fraction of the
/// image, and a reader needs to know which image. Without them Lightroom
/// ignores the whole block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Regions {
    pub width: u32,
    pub height: u32,
    pub faces: Vec<photosite_core::people::Region>,
}

impl Xmp {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Both halves of a position have arrived, so make one of them.
    fn settle_place(&mut self) {
        if let (Some(latitude), Some(longitude)) = (&self.latitude, &self.longitude) {
            self.place = photosite_core::place::from_xmp(latitude, longitude);
        }
    }

    /// What the catalogue would hold if this were all we knew.
    pub fn as_organisation(&self) -> Organisation {
        Organisation {
            rating: self.rating.unwrap_or(0).min(Organisation::MAX_RATING),
            label: self
                .label
                .as_deref()
                .map(ColorLabel::from_xmp_name)
                .unwrap_or_default(),
            // The verdict is not an XMP property. Lightroom keeps its picks
            // in its own catalogue and so do we; there is nothing standard
            // to read here and inventing one would be a private dialect.
            flag: Default::default(),
            title: self.title.clone(),
            description: self.description.clone(),
            keywords: self.keywords.clone(),
        }
    }
}

impl From<&Organisation> for Xmp {
    fn from(organisation: &Organisation) -> Self {
        Self {
            rating: (organisation.rating > 0).then_some(organisation.rating),
            label: (organisation.label != ColorLabel::None)
                .then(|| organisation.label.xmp_name().to_owned()),
            title: organisation.title.clone(),
            description: organisation.description.clone(),
            keywords: organisation.keywords.clone(),
            // A position is not part of what somebody said about a
            // photograph; it is set beside it, by whoever knows one. The
            // same goes for who is on it.
            place: None,
            regions: None,
            latitude: None,
            longitude: None,
        }
    }
}

/// Is this one of the properties we own?
///
/// The face regions are ours **only when we have some to write**. A
/// photograph nobody has face-scanned here may well carry frames another
/// program put there, and taking those out because we happen to be writing a
/// rating would be destroying somebody else's work on the way past.
fn ours(namespace: Option<&[u8]>, local: &[u8], regions: bool) -> bool {
    match namespace {
        Some(NS_XMP) => matches!(local, b"Rating" | b"Label"),
        Some(NS_DC) => matches!(local, b"title" | b"description" | b"subject"),
        Some(NS_EXIF) => matches!(local, b"GPSLatitude" | b"GPSLongitude"),
        Some(NS_MWG_RS) => regions && local == b"Regions",
        _ => false,
    }
}

/// Reads the five properties out of a packet.
///
/// Anything it cannot make sense of comes back empty rather than as an
/// error. A packet is somebody else's writing and may be anything at all;
/// refusing to show a photograph because its metadata is odd would be the
/// wrong trade every time.
pub fn read(packet: &str) -> Xmp {
    let mut found = Xmp::default();
    let mut reader = NsReader::from_str(packet);
    // Deliberately not trimming: the reader hands text back in pieces around
    // every entity, and trimming each piece eats the spaces between them —
    // "Bell & Ross" came back as "Bell&Ross". The whole value is trimmed
    // once, when it is committed.

    // Which of our properties we are inside, if any, and what has been
    // collected for the innermost element.
    //
    // The value is collected rather than taken from the first text event,
    // because a reader hands text back in pieces: `Bell &amp; Ross` arrives
    // as text, an entity reference, and more text. Taking the first piece
    // gave "Bell". Committing on each closing tag is also what separates one
    // keyword from the next — every `rdf:li` in the bag ends with one.
    let mut inside: Option<Property> = None;
    let mut depth = 0usize;
    let mut collected = String::new();

    loop {
        // The event and its namespace both borrow the reader, and resolving
        // an attribute needs the reader again. So each step is copied out
        // first and looked at afterwards; a packet is a few kilobytes and
        // the copy costs nothing worth counting.
        let step = match reader.read_resolved_event() {
            Ok((resolved, event)) => (
                namespace_of(&resolved).map(<[u8]>::to_vec),
                event.into_owned(),
            ),
            Err(error) => {
                tracing::debug!(%error, "the XMP packet could not be read to the end");
                break;
            }
        };

        let (namespace, event) = step;
        let namespace = namespace.as_deref();
        match event {
            Event::Start(element) => {
                if inside.is_some() {
                    depth += 1;
                    collected.clear();
                    continue;
                }

                // The shorthand form: the property sits as an attribute on
                // rdf:Description rather than as a child element. Adobe
                // writes packets this way and a reader that only knows the
                // long form finds nothing in them.
                read_attributes(&mut found, &reader, &element);

                let local = element.local_name().as_ref().to_vec();
                if let Some(property) = Property::of(namespace, &local) {
                    inside = Some(property);
                    depth = 0;
                    collected.clear();
                }
            }
            Event::Empty(element) => read_attributes(&mut found, &reader, &element),
            Event::Text(text) => {
                if inside.is_some()
                    && let Ok(value) = text.decode()
                {
                    collected.push_str(&value);
                }
            }
            Event::CData(data) => {
                if inside.is_some()
                    && let Ok(value) = data.decode()
                {
                    collected.push_str(&value);
                }
            }
            Event::GeneralRef(reference) => {
                // The event carries the entity's *name*, not what it stands
                // for. Handing the whole reference back to the unescaper is
                // what turns `amp` into `&` — and it covers the numeric
                // forms too, which a table written here would not.
                if inside.is_some()
                    && let Ok(name) = reference.decode()
                    && let Ok(value) = quick_xml::escape::unescape(&format!("&{name};"))
                {
                    collected.push_str(&value);
                }
            }
            Event::End(_) => {
                if let Some(property) = inside {
                    let value = collected.trim().to_owned();
                    if !value.is_empty() {
                        property.take(&mut found, value);
                    }

                    collected.clear();
                    if depth == 0 {
                        inside = None;
                    } else {
                        depth -= 1;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Property {
    Rating,
    Label,
    Title,
    Description,
    Subject,
    Latitude,
    Longitude,
}

impl Property {
    fn of(namespace: Option<&[u8]>, local: &[u8]) -> Option<Self> {
        match (namespace, local) {
            (Some(NS_XMP), b"Rating") => Some(Self::Rating),
            (Some(NS_XMP), b"Label") => Some(Self::Label),
            (Some(NS_DC), b"title") => Some(Self::Title),
            (Some(NS_DC), b"description") => Some(Self::Description),
            (Some(NS_DC), b"subject") => Some(Self::Subject),
            (Some(NS_EXIF), b"GPSLatitude") => Some(Self::Latitude),
            (Some(NS_EXIF), b"GPSLongitude") => Some(Self::Longitude),
            _ => None,
        }
    }

    /// Puts a value where it belongs. Keywords accumulate; the rest take the
    /// first one seen, because a well-formed packet has one of each and a
    /// malformed one is not worth a preference.
    fn take(self, into: &mut Xmp, value: String) {
        match self {
            Self::Rating => {
                if into.rating.is_none() {
                    into.rating = value.trim().parse::<f32>().ok().map(|number| {
                        number.round().clamp(0.0, Organisation::MAX_RATING as f32) as u8
                    });
                }
            }
            Self::Label => {
                into.label.get_or_insert(value);
            }
            Self::Latitude => {
                into.latitude.get_or_insert(value);
                into.settle_place();
            }
            Self::Longitude => {
                into.longitude.get_or_insert(value);
                into.settle_place();
            }
            Self::Title => {
                into.title.get_or_insert(value);
            }
            Self::Description => {
                into.description.get_or_insert(value);
            }
            Self::Subject => {
                if !into
                    .keywords
                    .iter()
                    .any(|had| had.eq_ignore_ascii_case(&value))
                {
                    into.keywords.push(value);
                }
            }
        }
    }
}

fn namespace_of<'a>(resolved: &'a quick_xml::name::ResolveResult<'_>) -> Option<&'a [u8]> {
    match resolved {
        quick_xml::name::ResolveResult::Bound(namespace) => Some(namespace.as_ref()),
        _ => None,
    }
}

/// The shorthand form, where a property is an attribute of `rdf:Description`.
fn read_attributes(found: &mut Xmp, reader: &NsReader<&[u8]>, element: &BytesStart<'_>) {
    for attribute in element.attributes().flatten() {
        let (resolved, local) = reader.resolve_attribute(attribute.key);
        let Some(property) = Property::of(namespace_of(&resolved), local.as_ref()) else {
            continue;
        };

        if let Ok(value) = attribute.decode_and_unescape_value(reader.decoder()) {
            let value = value.trim().to_owned();
            if !value.is_empty() {
                property.take(found, value);
            }
        }
    }
}

/// The bytes that open and close a packet in a file.
const PACKET_OPEN: &str = "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n";
const PACKET_CLOSE: &str = "\n<?xpacket end=\"w\"?>";

/// Ours, as one `rdf:Description` block.
fn our_block(xmp: &Xmp) -> String {
    let mut out = String::from(
        "  <rdf:Description rdf:about=\"\"\n      \
         xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n      \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n      \
         xmlns:exif=\"http://ns.adobe.com/exif/1.0/\">\n",
    );

    if let Some(rating) = xmp.rating {
        out.push_str(&format!("   <xmp:Rating>{rating}</xmp:Rating>\n"));
    }

    if let Some(label) = &xmp.label {
        out.push_str(&format!("   <xmp:Label>{}</xmp:Label>\n", escape(label)));
    }

    // Title and description are language alternatives, not plain text. Every
    // cataloguer reads `x-default`; a bare string here is read by some and
    // ignored by others.
    for (name, value) in [("title", &xmp.title), ("description", &xmp.description)] {
        if let Some(value) = value {
            out.push_str(&format!(
                "   <dc:{name}>\n    <rdf:Alt>\n     \
                 <rdf:li xml:lang=\"x-default\">{}</rdf:li>\n    \
                 </rdf:Alt>\n   </dc:{name}>\n",
                escape(value)
            ));
        }
    }

    if let Some(place) = xmp.place {
        let (latitude, longitude) = place.as_xmp();
        out.push_str(&format!(
            "   <exif:GPSLatitude>{latitude}</exif:GPSLatitude>\n   \
             <exif:GPSLongitude>{longitude}</exif:GPSLongitude>\n"
        ));
    }

    if !xmp.keywords.is_empty() {
        out.push_str("   <dc:subject>\n    <rdf:Bag>\n");
        for keyword in &xmp.keywords {
            out.push_str(&format!("     <rdf:li>{}</rdf:li>\n", escape(keyword)));
        }

        out.push_str("    </rdf:Bag>\n   </dc:subject>\n");
    }

    out.push_str("  </rdf:Description>\n");
    if let Some(regions) = &xmp.regions {
        out.push_str(&region_block(regions));
    }

    out
}

/// The face frames, as the Metadata Working Group specifies them.
///
/// A block of its own rather than more properties in the one above, because
/// it is a nested structure carrying two further namespaces and mixing it in
/// would make both harder to read. A second `rdf:Description` is legal and
/// is what exiftool writes too.
///
/// Three details decide whether anybody else can read the result:
///
/// * **An area is centred**, not measured from its corner. Everything else
///   in this application measures a rectangle from its top left, so the
///   conversion happens here, once, at the boundary — a reader that assumes
///   otherwise draws every frame offset by half its own size.
/// * **The applied-to dimensions must be there.** A fraction of an image
///   means nothing without saying which image, and Lightroom ignores a
///   region list that does not carry them.
/// * **The numbers are written with a full stop** and never by the locale's
///   rules. The same trap as a coordinate in a URL, and the same answer.
fn region_block(regions: &Regions) -> String {
    let mut out = String::from(
        "  <rdf:Description rdf:about=\"\"
               xmlns:mwg-rs=\"http://www.metadataworkinggroup.com/schemas/regions/\"
               xmlns:stArea=\"http://ns.adobe.com/xmp/sType/Area#\"
               xmlns:stDim=\"http://ns.adobe.com/xap/1.0/sType/Dimensions#\">
            <mwg-rs:Regions rdf:parseType=\"Resource\">
",
    );
    out.push_str(&format!(
        "    <mwg-rs:AppliedToDimensions rdf:parseType=\"Resource\">
              <stDim:w>{}</stDim:w>
              <stDim:h>{}</stDim:h>
              <stDim:unit>pixel</stDim:unit>
             </mwg-rs:AppliedToDimensions>
",
        regions.width, regions.height
    ));

    out.push_str(
        "    <mwg-rs:RegionList>
     <rdf:Bag>
",
    );
    for face in &regions.faces {
        let centre = |start: f64, size: f64| number(start + size / 2.0);
        out.push_str(&format!(
            "      <rdf:li rdf:parseType=\"Resource\">
                    <mwg-rs:Name>{}</mwg-rs:Name>
                    <mwg-rs:Type>Face</mwg-rs:Type>
                    <mwg-rs:Area rdf:parseType=\"Resource\">
                     <stArea:x>{}</stArea:x>
                     <stArea:y>{}</stArea:y>
                     <stArea:w>{}</stArea:w>
                     <stArea:h>{}</stArea:h>
                     <stArea:unit>normalized</stArea:unit>
                    </mwg-rs:Area>
                   </rdf:li>
",
            escape(&face.name),
            centre(face.x, face.width),
            centre(face.y, face.height),
            number(face.width),
            number(face.height),
        ));
    }

    out.push_str(
        "     </rdf:Bag>
    </mwg-rs:RegionList>
   </mwg-rs:Regions>
",
    );
    out.push_str(
        "  </rdf:Description>
",
    );
    out
}

/// A fraction, clamped into the frame and written with a full stop.
fn number(value: f64) -> String {
    format!("{:.6}", value.clamp(0.0, 1.0))
}

fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

/// A packet from nothing.
fn fresh(xmp: &Xmp) -> String {
    format!(
        "{PACKET_OPEN}<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"PhotoSite\">\n \
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         {}\
         </rdf:RDF>\n</x:xmpmeta>{PACKET_CLOSE}",
        our_block(xmp)
    )
}

/// Puts ours into a packet, keeping everything that is not ours.
///
/// Our properties are taken out of wherever they were — child elements or
/// attributes, under any prefix — and written back as one block. Anything
/// else in the packet is copied through: other namespaces, other
/// `rdf:Description` blocks, whatever a camera or another cataloguer left
/// behind.
pub fn merge(existing: Option<&str>, xmp: &Xmp) -> String {
    let Some(existing) = existing else {
        return fresh(xmp);
    };

    match transform(existing, xmp) {
        Ok(packet) => packet,
        Err(error) => {
            // A packet we cannot even walk is not one we can safely edit, so
            // we say so and start a clean one rather than writing something
            // half-transformed into somebody's photograph.
            tracing::warn!(%error, "the XMP packet could not be rewritten; starting a fresh one");
            fresh(xmp)
        }
    }
}

fn transform(existing: &str, xmp: &Xmp) -> anyhow::Result<String> {
    // Whether the face frames already in the packet are ours to replace.
    let replacing_regions = xmp.regions.is_some();
    let mut reader = NsReader::from_str(existing);
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    let mut skipping: Option<usize> = None;
    let mut wrote_ours = false;
    // Taking our properties out of an `rdf:Description` can leave it with
    // nothing in it. An empty one is legal and harmless, but it is debris,
    // and debris left in somebody's photograph on every save is the kind of
    // thing that is noticed years later. So each one is held back until it
    // is known whether anything is left inside.
    let mut held: Option<Held> = None;
    // The indentation in front of a block is held back with it. Dropping an
    // emptied block on its own leaves its leading newline behind, and the
    // next save leaves another: the packet grows a little every time it is
    // written, forever. `writing_twice_settles_rather_than_growing` is what
    // found it.
    let mut pending_space: Option<String> = None;

    loop {
        let (namespace, event) = {
            let (resolved, event) = reader.read_resolved_event()?;
            (
                namespace_of(&resolved).map(<[u8]>::to_vec),
                event.into_owned(),
            )
        };
        let namespace = namespace.as_deref();

        // Inside one of our properties: drop the whole subtree.
        if let Some(depth) = skipping {
            match event {
                Event::Start(_) => skipping = Some(depth + 1),
                Event::End(_) => {
                    if depth == 0 {
                        skipping = None;
                    } else {
                        skipping = Some(depth - 1);
                    }
                }
                Event::Eof => break,
                _ => {}
            }

            continue;
        }

        // Whitespace between elements waits to see what follows it.
        if held.is_none()
            && let Event::Text(text) = &event
            && let Ok(value) = text.decode()
            && value.trim().is_empty()
        {
            pending_space
                .get_or_insert_with(String::new)
                .push_str(&value);
            continue;
        }

        match event {
            Event::Start(element) => {
                let local = element.local_name().as_ref().to_vec();
                if ours(namespace, &local, replacing_regions) {
                    skipping = Some(0);
                    continue;
                }

                let kept = without_our_attributes(&reader, &element, replacing_regions);
                if held.is_none() && namespace == Some(NS_RDF) && local == b"Description" {
                    held = Some(Held {
                        start: kept,
                        inner: Writer::new(Cursor::new(Vec::new())),
                        anything: false,
                        depth: 0,
                        space: pending_space.take().unwrap_or_default(),
                    });
                    continue;
                }

                flush(&mut writer, &mut pending_space)?;
                if let Some(holder) = held.as_mut() {
                    holder.depth += 1;
                    holder.anything = true;
                    holder.inner.write_event(Event::Start(kept))?;
                } else {
                    writer.write_event(Event::Start(kept))?;
                }
            }
            Event::Empty(element) => {
                let local = element.local_name().as_ref().to_vec();
                if ours(namespace, &local, replacing_regions) {
                    continue;
                }

                let kept = without_our_attributes(&reader, &element, replacing_regions);
                if held.is_none() && namespace == Some(NS_RDF) && local == b"Description" {
                    // An empty Description with nothing of ours left on it
                    // says nothing at all.
                    if !says_anything(&kept) {
                        continue;
                    }
                }

                flush(&mut writer, &mut pending_space)?;
                if let Some(holder) = held.as_mut() {
                    holder.anything = true;
                    holder.inner.write_event(Event::Empty(kept))?;
                } else {
                    writer.write_event(Event::Empty(kept))?;
                }
            }
            Event::End(element) => {
                let local = element.local_name().as_ref().to_vec();
                if let Some(holder) = held.as_mut() {
                    if holder.depth > 0 {
                        holder.depth -= 1;
                        holder
                            .inner
                            .write_event(Event::End(BytesEnd::new(name_of(&element))))?;
                        continue;
                    }

                    let holder = held.take().expect("held just checked");
                    if holder.anything || says_anything(&holder.start) {
                        let name = name_of_start(&holder.start);
                        writer.write_event(Event::Text(BytesText::from_escaped(holder.space)))?;
                        writer.write_event(Event::Start(holder.start))?;
                        writer.write_event(Event::Text(BytesText::from_escaped(
                            String::from_utf8(holder.inner.into_inner().into_inner())?,
                        )))?;
                        writer.write_event(Event::End(BytesEnd::new(name)))?;
                    }

                    continue;
                }

                flush(&mut writer, &mut pending_space)?;

                // Ours go in just before the RDF closes, so they land inside
                // it however the rest of the packet is arranged.
                if namespace == Some(NS_RDF) && local == b"RDF" {
                    let block = our_block(xmp);
                    writer.write_event(Event::Text(BytesText::from_escaped(block)))?;
                    wrote_ours = true;
                }

                writer.write_event(Event::End(BytesEnd::new(name_of(&element))))?;
            }
            Event::Eof => break,
            other => {
                flush(&mut writer, &mut pending_space)?;
                if let Some(holder) = held.as_mut() {
                    if let Event::Text(text) = &other
                        && let Ok(value) = text.decode()
                        && !value.trim().is_empty()
                    {
                        holder.anything = true;
                    }

                    holder.inner.write_event(other)?;
                } else {
                    writer.write_event(other)?;
                }
            }
        }
    }

    let out = String::from_utf8(writer.into_inner().into_inner())?;
    anyhow::ensure!(wrote_ours, "the packet has no rdf:RDF to write into");
    Ok(out)
}

/// An `rdf:Description` being held back until it is known whether taking our
/// properties out of it left anything behind.
struct Held {
    start: BytesStart<'static>,
    inner: Writer<Cursor<Vec<u8>>>,
    /// Whether anything was written inside it.
    anything: bool,
    depth: usize,
    /// The whitespace that stood in front of it, which goes wherever it
    /// goes.
    space: String,
}

/// Lets out whitespace that was waiting to see what came next.
fn flush(writer: &mut Writer<Cursor<Vec<u8>>>, pending: &mut Option<String>) -> anyhow::Result<()> {
    if let Some(space) = pending.take() {
        writer.write_event(Event::Text(BytesText::from_escaped(space)))?;
    }

    Ok(())
}

/// Does this element carry anything beyond the bookkeeping?
///
/// `rdf:about` and the namespace declarations are not content: a Description
/// holding only those is an empty one however many attributes it has.
fn says_anything(element: &BytesStart<'_>) -> bool {
    element.attributes().flatten().any(|attribute| {
        let key = attribute.key.as_ref();
        key != b"rdf:about" && !key.starts_with(b"xmlns")
    })
}

fn name_of(element: &BytesEnd<'_>) -> String {
    String::from_utf8_lossy(element.name().as_ref()).into_owned()
}

fn name_of_start(element: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(element.name().as_ref()).into_owned()
}

/// The same element with any of our properties stripped from its attributes.
fn without_our_attributes(
    reader: &NsReader<&[u8]>,
    element: &BytesStart<'_>,
    regions: bool,
) -> BytesStart<'static> {
    let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
    let mut kept = BytesStart::new(name);
    for attribute in element.attributes().flatten() {
        let (resolved, local) = reader.resolve_attribute(attribute.key);
        if ours(namespace_of(&resolved), local.as_ref(), regions) {
            continue;
        }

        kept.push_attribute(attribute);
    }

    kept.into_owned()
}

/// Wraps a packet for a `.xmp` file beside the photograph.
pub fn sidecar(xmp: &Xmp) -> String {
    fresh(xmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Xmp {
        Xmp {
            rating: Some(4),
            label: Some("Green".to_owned()),
            title: Some("Sunrise".to_owned()),
            description: Some("The first morning".to_owned()),
            keywords: vec!["Hawaii".to_owned(), "holiday".to_owned()],
            place: None,
            regions: None,
            latitude: None,
            longitude: None,
        }
    }

    fn face(name: &str, x: f64, y: f64, size: f64) -> photosite_core::people::Region {
        photosite_core::people::Region {
            name: name.to_owned(),
            x,
            y,
            width: size,
            height: size,
        }
    }

    /// An MWG area is measured from its **centre**. Everything else here
    /// measures from a corner, so this conversion is the one thing in the
    /// block that can be silently wrong — and it would show as every face
    /// frame in Lightroom sitting half a face up and to the left.
    #[test]
    fn a_face_frame_is_written_from_its_centre() {
        let xmp = Xmp {
            regions: Some(Regions {
                width: 6000,
                height: 4000,
                faces: vec![face("Jana", 0.2, 0.3, 0.1)],
            }),
            ..Default::default()
        };

        let packet = merge(None, &xmp);
        assert!(packet.contains("<stArea:x>0.250000</stArea:x>"), "{packet}");
        assert!(packet.contains("<stArea:y>0.350000</stArea:y>"), "{packet}");
        assert!(packet.contains("<stArea:w>0.100000</stArea:w>"), "{packet}");
        assert!(
            packet.contains("<mwg-rs:Name>Jana</mwg-rs:Name>"),
            "{packet}"
        );
        assert!(
            packet.contains("<mwg-rs:Type>Face</mwg-rs:Type>"),
            "{packet}"
        );
    }

    /// A fraction of an image means nothing without saying which image.
    #[test]
    fn the_frame_the_faces_were_measured_in_goes_with_them() {
        let xmp = Xmp {
            regions: Some(Regions {
                width: 6000,
                height: 4000,
                faces: vec![face("Jana", 0.2, 0.3, 0.1)],
            }),
            ..Default::default()
        };

        let packet = merge(None, &xmp);
        assert!(packet.contains("<stDim:w>6000</stDim:w>"), "{packet}");
        assert!(packet.contains("<stDim:h>4000</stDim:h>"), "{packet}");
        assert!(
            packet.contains("<stDim:unit>pixel</stDim:unit>"),
            "{packet}"
        );
    }

    /// The rule that keeps us from destroying somebody else's work on the
    /// way past. A photograph nobody has swept here keeps the face frames
    /// Lightroom or Picasa put in it, even while we write a rating.
    #[test]
    fn face_frames_we_know_nothing_about_are_left_alone() {
        let existing = concat!(
            "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>",
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">",
            "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
            "<rdf:Description rdf:about=\"\"",
            " xmlns:mwg-rs=\"http://www.metadataworkinggroup.com/schemas/regions/\">",
            "<mwg-rs:Regions rdf:parseType=\"Resource\">",
            "<mwg-rs:RegionList><rdf:Bag><rdf:li rdf:parseType=\"Resource\">",
            "<mwg-rs:Name>Somebody Else</mwg-rs:Name>",
            "</rdf:li></rdf:Bag></mwg-rs:RegionList>",
            "</mwg-rs:Regions></rdf:Description>",
            "</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
        );

        let rating = Xmp {
            rating: Some(4),
            ..Default::default()
        };
        assert_eq!(rating.regions, None, "the fixture must not claim to know");
        let packet = merge(Some(existing), &rating);
        assert!(
            packet.contains("Somebody Else"),
            "another program's face frames were thrown away: {packet}"
        );
        assert!(packet.contains("<xmp:Rating>4</xmp:Rating>"), "{packet}");
    }

    /// And the other half of the same rule: once we have looked, ours are
    /// the answer — including the answer "nobody here is named", which is
    /// how a face taken off somebody comes back out of the file.
    #[test]
    fn once_we_have_looked_our_face_frames_replace_what_was_there() {
        let named = Xmp {
            regions: Some(Regions {
                width: 100,
                height: 100,
                faces: vec![face("Jana", 0.1, 0.1, 0.2)],
            }),
            ..Default::default()
        };
        let packet = merge(None, &named);
        assert!(packet.contains("Jana"));

        let cleared = Xmp {
            regions: Some(Regions {
                width: 100,
                height: 100,
                faces: Vec::new(),
            }),
            ..Default::default()
        };
        let packet = merge(Some(&packet), &cleared);
        assert!(!packet.contains("Jana"), "the name stayed behind: {packet}");
    }

    /// The packet must not grow a block every time it is written, which is
    /// the same trap the whitespace handling was written for.
    #[test]
    fn writing_the_faces_twice_settles_rather_than_growing() {
        let xmp = Xmp {
            regions: Some(Regions {
                width: 100,
                height: 100,
                faces: vec![face("Jana", 0.1, 0.1, 0.2)],
            }),
            ..Default::default()
        };

        let once = merge(None, &xmp);
        let twice = merge(Some(&once), &xmp);
        let thrice = merge(Some(&twice), &xmp);
        assert_eq!(twice, thrice, "the packet is still changing");
        assert_eq!(twice.matches("mwg-rs:Regions").count(), 2, "{twice}");
    }

    /// A name with an ampersand in it is a name, not a broken packet.
    #[test]
    fn a_name_with_markup_in_it_is_escaped() {
        let xmp = Xmp {
            regions: Some(Regions {
                width: 10,
                height: 10,
                faces: vec![face("Bell & <Ross>", 0.1, 0.1, 0.2)],
            }),
            ..Default::default()
        };

        let packet = merge(None, &xmp);
        assert!(packet.contains("Bell &amp; &lt;Ross&gt;"), "{packet}");
        // And it still parses, which is the point of escaping it.
        assert_eq!(read(&packet).rating, None);
    }

    #[test]
    fn what_we_write_is_what_we_read_back() {
        let packet = merge(None, &sample());
        assert_eq!(read(&packet), sample());
    }

    #[test]
    fn an_empty_set_makes_a_packet_that_says_nothing() {
        let packet = merge(None, &Xmp::default());
        assert!(read(&packet).is_empty());
    }

    /// The packet v1 wrote, with the old `xap:` prefix for the very same
    /// namespace. A reader that matched on the prefix would find nothing.
    #[test]
    fn the_old_prefix_is_the_same_namespace() {
        let packet = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 4.1.1">
   <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about="" xmlns:xap="http://ns.adobe.com/xap/1.0/">
         <xap:Rating>5</xap:Rating>
      </rdf:Description>
      <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/">
         <dc:title>
            <rdf:Alt>
               <rdf:li xml:lang="x-default">Cestou na letiste</rdf:li>
            </rdf:Alt>
         </dc:title>
      </rdf:Description>
   </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;
        let read = read(packet);
        assert_eq!(read.rating, Some(5));
        assert_eq!(read.title.as_deref(), Some("Cestou na letiste"));
    }

    /// Adobe writes properties as attributes. Half the packets in the world
    /// look like this and none of them have a child element to find.
    #[test]
    fn the_shorthand_form_is_read_too() {
        let packet = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmp:Rating="3"
    xmp:Label="Red"/>
 </rdf:RDF>
</x:xmpmeta>"#;
        let read = read(packet);
        assert_eq!(read.rating, Some(3));
        assert_eq!(read.label.as_deref(), Some("Red"));
    }

    /// The one that matters most in this file. Somebody else's properties
    /// are not ours to throw away.
    #[test]
    fn merging_keeps_what_is_not_ours() {
        let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmp:Rating="1"
    crs:Exposure2012="+0.35">
   <crs:ToneCurveName>Strong Contrast</crs:ToneCurveName>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;

        let merged = merge(Some(existing), &sample());
        assert!(
            merged.contains("Exposure2012"),
            "somebody else's attribute went missing:\n{merged}"
        );
        assert!(
            merged.contains("Strong Contrast"),
            "somebody else's element went missing:\n{merged}"
        );
        assert_eq!(read(&merged), sample(), "ours did not land");
        assert!(
            !merged.contains("xmp:Rating=\"1\""),
            "the old rating is still there as an attribute:\n{merged}"
        );
    }

    #[test]
    fn a_block_left_empty_by_the_merge_is_taken_out() {
        // Exactly the shape v1 wrote: one block per namespace, each holding
        // one of our properties and nothing else.
        let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xap="http://ns.adobe.com/xap/1.0/">
   <xap:Rating>5</xap:Rating>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;

        let merged = merge(Some(existing), &sample());
        assert_eq!(read(&merged), sample());
        assert_eq!(
            merged.matches("<rdf:Description").count(),
            1,
            "an empty block was left behind:\n{merged}"
        );
    }

    #[test]
    fn a_block_that_still_holds_something_is_kept() {
        let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xap="http://ns.adobe.com/xap/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/">
   <xap:Rating>5</xap:Rating>
   <tiff:Make>NIKON CORPORATION</tiff:Make>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;

        let merged = merge(Some(existing), &sample());
        assert!(merged.contains("NIKON CORPORATION"), "{merged}");
        assert!(!merged.contains("<xap:Rating>"), "{merged}");
        assert_eq!(read(&merged).rating, Some(4));
    }

    #[test]
    fn merging_replaces_rather_than_adding_a_second_copy() {
        let once = merge(None, &sample());
        let twice = merge(Some(&once), &sample());
        assert_eq!(read(&twice), sample());
        assert_eq!(
            twice.matches("<xmp:Rating>").count(),
            1,
            "the rating was written twice:\n{twice}"
        );
        assert_eq!(twice.matches("<dc:subject>").count(), 1);
    }

    #[test]
    fn clearing_a_value_takes_it_out_of_the_packet() {
        let once = merge(None, &sample());
        let cleared = merge(Some(&once), &Xmp::default());
        assert!(read(&cleared).is_empty(), "{cleared}");
        assert!(!cleared.contains("Sunrise"), "{cleared}");
    }

    #[test]
    fn keywords_survive_the_round_trip_in_order() {
        let packet = merge(None, &sample());
        assert_eq!(read(&packet).keywords, ["Hawaii", "holiday"]);
    }

    #[test]
    fn text_that_would_break_the_xml_is_escaped() {
        let awkward = Xmp {
            title: Some("Bell & Ross <5> \"quoted\"".to_owned()),
            ..Default::default()
        };
        let packet = merge(None, &awkward);
        assert_eq!(read(&packet).title, awkward.title);
    }

    #[test]
    fn a_packet_we_cannot_walk_does_not_lose_what_we_are_writing() {
        let nonsense = "<not really <<< xml";
        let packet = merge(Some(nonsense), &sample());
        assert_eq!(read(&packet), sample());
    }

    #[test]
    fn a_rating_written_as_a_decimal_is_still_a_rating() {
        // Some tools write 3.0 rather than 3.
        let packet = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmp:Rating="3.0"/>
 </rdf:RDF>
</x:xmpmeta>"#;
        assert_eq!(read(packet).rating, Some(3));
    }

    #[test]
    fn a_label_travels_through_the_domain_and_back() {
        let xmp = Xmp::from(&Organisation {
            label: ColorLabel::Purple,
            ..Default::default()
        });
        assert_eq!(xmp.label.as_deref(), Some("Purple"));
        assert_eq!(xmp.as_organisation().label, ColorLabel::Purple);
    }

    #[test]
    fn the_verdict_is_not_an_xmp_property_and_does_not_pretend_to_be() {
        let packet = merge(
            None,
            &Xmp::from(&Organisation {
                flag: photosite_core::domain::Flag::Picked,
                rating: 2,
                ..Default::default()
            }),
        );
        assert!(!packet.to_lowercase().contains("pick"), "{packet}");
        assert_eq!(read(&packet).rating, Some(2));
    }
}
