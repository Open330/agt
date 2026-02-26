---
name: processing-documents
description: Processes PDF, DOCX, XLSX, PPTX, HWP, HWPX documents including analysis, summarization, and format conversion. Use for "문서 분석", "PDF 변환", "Excel 추출", "문서 요약", "HWP 변환", "HWPX 변환", "한글 문서" requests or when working with office documents.
---

# Document Processor

Analyze, summarize, and convert office documents.

## Supported Formats

| Format | Read | Write | Tools |
|--------|------|-------|-------|
| PDF | ✅ | ✅ | pdfplumber, pypdf |
| DOCX | ✅ | ✅ | python-docx |
| XLSX | ✅ | ✅ | openpyxl |
| PPTX | ✅ | ✅ | python-pptx |
| HWP | ✅ | ❌ | python-hwp, olefile |
| HWPX | ✅ | ❌ | zipfile, xml.etree |

## Quick Reference

### PDF Text Extraction

```python
import pdfplumber
with pdfplumber.open("doc.pdf") as pdf:
    text = "\n".join(p.extract_text() for p in pdf.pages)
```

### Excel Reading

```python
import openpyxl
wb = openpyxl.load_workbook("data.xlsx")
ws = wb.active
data = [[cell.value for cell in row] for row in ws.iter_rows()]
```

### Word Document

```python
from docx import Document
doc = Document("report.docx")
text = "\n".join(p.text for p in doc.paragraphs)
```

### HWP Document (한글)

```python
# Using pyhwp (hwp5) - primary library
from hwp5.xmlmodel import Hwp5File

hwp = Hwp5File("document.hwp")
for section in hwp.bodytext:
    # Process paragraphs in each section
    pass
```

```python
# Using olefile - low-level OLE access fallback
import olefile

ole = olefile.OleFileIO("document.hwp")
# HWP stores text in "BodyText/Section0", "BodyText/Section1", etc.
streams = [s for s in ole.listdir() if s[0] == "BodyText"]
for stream_path in streams:
    data = ole.openstream(stream_path).read()
    # HWP uses UTF-16LE encoding for text content
    text = data.decode("utf-16-le", errors="ignore")
```

### HWPX Document (한글 OOXML)

```python
# HWPX is a ZIP containing XML files
import zipfile
import xml.etree.ElementTree as ET

def hwpx_to_text(path: str) -> str:
    texts = []
    with zipfile.ZipFile(path) as zf:
        # HWPX stores content in Contents/section*.xml
        section_files = sorted(
            [n for n in zf.namelist() if n.startswith("Contents/section") and n.endswith(".xml")]
        )
        for section_file in section_files:
            with zf.open(section_file) as f:
                tree = ET.parse(f)
                root = tree.getroot()
                # Extract text from all <hp:t> elements
                ns = {"hp": "http://www.hancom.co.kr/hwpml/2011/paragraph"}
                for t_elem in root.iter("{http://www.hancom.co.kr/hwpml/2011/paragraph}t"):
                    if t_elem.text:
                        texts.append(t_elem.text)
    return "\n".join(texts)
```

## Workflows

### Summarize PDF
1. Extract text with pdfplumber
2. Pass to Claude for summarization
3. Output markdown summary

### Convert Excel to CSV
```python
import pandas as pd
df = pd.read_excel("data.xlsx")
df.to_csv("data.csv", index=False)
```

### Extract Tables from PDF
```python
with pdfplumber.open("doc.pdf") as pdf:
    tables = pdf.pages[0].extract_tables()
```

### Convert HWP to Text/Markdown
```python
import olefile

def hwp_to_text(path: str) -> str:
    ole = olefile.OleFileIO(path)
    streams = sorted(
        [s for s in ole.listdir() if s[0] == "BodyText"],
        key=lambda s: s[1]  # sort by section number
    )
    sections = []
    for stream_path in streams:
        data = ole.openstream(stream_path).read()
        text = data.decode("utf-16-le", errors="ignore")
        # Strip null bytes and control characters
        text = "".join(c for c in text if c.isprintable() or c in "\n\t")
        sections.append(text.strip())
    return "\n\n".join(sections)

text = hwp_to_text("document.hwp")
# Pass text to Claude for summarization or markdown conversion
```

### Convert HWPX to Text/Markdown
```python
# HWPX conversion (simpler than HWP - standard ZIP+XML)
text = hwpx_to_text("document.hwpx")
# Pass to Claude for markdown conversion
```

## Best Practices

- Use pdfplumber for complex PDFs (tables, layouts)
- Use pypdf for simple text extraction
- Convert to markdown for AI processing
- For HWP files, prefer pyhwp (`pip install pyhwp`) for structured access; fall back to olefile for raw extraction
- HWP write support is not reliably available — convert to DOCX or PDF for output instead
- HWP text is stored in UTF-16LE encoding inside OLE compound streams; always specify `errors="ignore"` when decoding
- Sort BodyText sections by section index to preserve document order when using olefile directly
- HWPX is a ZIP+XML format (similar to DOCX); use `zipfile` and `xml.etree` from the standard library — no external dependencies needed
- Content sections are in `Contents/section0.xml`, `Contents/section1.xml`, etc.
- Prefer HWPX over HWP when both formats are available — HWPX is easier to parse and more reliable
