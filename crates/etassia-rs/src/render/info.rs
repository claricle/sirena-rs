use crate::Error;

/// The info diagram is a constant canvas showing the mermaid version.
const SVG: &str = "<svg\n width=\"400.0\"\n height=\"100.0\"\n viewBox=\"0 0 400 100\"\n version=\"1.2\"\n baseProfile=\"tiny\"\n xmlns=\"http://www.w3.org/2000/svg\"\n>\n<text fill=\"#000000\" x=\"100.0\" y=\"40.0\" text-anchor=\"middle\" font-family=\"Arial, Helvetica, sans-serif\" font-size=\"32\">v11.12.0</text>\n</svg>";

pub fn render(source: &str) -> Result<String, Error> {
    crate::parse::parse(source, "info")?;
    Ok(SVG.to_string())
}
