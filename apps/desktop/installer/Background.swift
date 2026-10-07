import AppKit

// The installer window's background, in the bar's palette. Icon wells sit
// under the positions in dmg.py.
guard CommandLine.arguments.count == 3 else {
  fatalError("Usage: swift Background.swift <apps/desktop> <output.tiff>")
}
let root = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
let width = 720
let height = 480

let ink: UInt32 = 0x0A0E0C
let fg: UInt32 = 0xE4EEE8
let muted: UInt32 = 0x92A59A
let emerald: UInt32 = 0x3B9B7A
let coral: UInt32 = 0xF07A6A

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> NSColor {
  NSColor(
    srgbRed: CGFloat((hex >> 16) & 255) / 255,
    green: CGFloat((hex >> 8) & 255) / 255,
    blue: CGFloat(hex & 255) / 255, alpha: alpha)
}

// Top-left coordinates, like the Finder positions.
func rect(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> NSRect {
  NSRect(x: x, y: CGFloat(height) - y - h, width: w, height: h)
}

func point(_ x: CGFloat, _ y: CGFloat) -> NSPoint {
  NSPoint(x: x, y: CGFloat(height) - y)
}

func rounded(_ size: CGFloat, _ weight: NSFont.Weight) -> NSFont {
  let font = NSFont.systemFont(ofSize: size, weight: weight)
  guard let descriptor = font.fontDescriptor.withDesign(.rounded) else { return font }
  return NSFont(descriptor: descriptor, size: size) ?? font
}

func text(
  _ value: String, x: CGFloat, y: CGFloat, width: CGFloat,
  font: NSFont, ink: UInt32, centered: Bool = false
) {
  let paragraph = NSMutableParagraphStyle()
  paragraph.alignment = centered ? .center : .left
  let attributes: [NSAttributedString.Key: Any] = [
    .font: font,
    .foregroundColor: color(ink),
    .paragraphStyle: paragraph,
  ]
  (value as NSString).draw(in: rect(x, y, width, 60), withAttributes: attributes)
}

func render(scale: Int) -> NSBitmapImageRep {
  let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: width * scale, pixelsHigh: height * scale,
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
    isPlanar: false, colorSpaceName: .deviceRGB,
    bytesPerRow: 0, bitsPerPixel: 0)!
  bitmap.size = NSSize(width: width, height: height)
  NSGraphicsContext.saveGraphicsState()
  NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)

  color(ink).setFill()
  NSBezierPath(rect: rect(0, 0, 720, 480)).fill()
  color(coral).setFill()
  NSBezierPath(rect: rect(0, 0, 720, 3)).fill()

  // Sound rings off the top right corner, quiet enough to keep the icons clear.
  for index in 0..<5 {
    let radius = CGFloat(90 + index * 46)
    let ring = NSBezierPath(
      ovalIn: NSRect(x: 720 - radius, y: 480 - radius, width: radius * 2, height: radius * 2))
    color(emerald, 0.16 - CGFloat(index) * 0.025).setStroke()
    ring.lineWidth = 1.5
    ring.stroke()
  }

  let icon = NSImage(contentsOf: root.appendingPathComponent("src-tauri/icons/icon.png"))!
  icon.draw(in: rect(44, 32, 52, 52))
  text("Beep", x: 108, y: 38, width: 260, font: rounded(30, .bold), ink: fg)
  text(
    "Your assistant, one tap away.", x: 48, y: 112, width: 624,
    font: rounded(28, .semibold), ink: fg)
  text(
    "Double-tap Option anywhere on this Mac to ask Beep.",
    x: 48, y: 154, width: 624, font: rounded(16, .regular), ink: muted)

  // Light wells, because Finder draws the icon labels in dark text.
  for x: CGFloat in [132, 452] {
    let well = NSBezierPath(roundedRect: rect(x, 204, 136, 148), xRadius: 28, yRadius: 28)
    color(fg).setFill()
    well.fill()
  }
  let arrow = NSBezierPath()
  arrow.move(to: point(306, 270))
  arrow.line(to: point(414, 270))
  arrow.move(to: point(403, 259))
  arrow.line(to: point(414, 270))
  arrow.line(to: point(403, 281))
  arrow.lineWidth = 2.5
  arrow.lineCapStyle = .round
  arrow.lineJoinStyle = .round
  color(emerald).setStroke()
  arrow.stroke()

  text(
    "Drag Beep into Applications", x: 48, y: 382, width: 624,
    font: rounded(20, .semibold), ink: fg, centered: true)
  text(
    "Then open it from Applications. It lives in the menu bar.",
    x: 48, y: 416, width: 624, font: rounded(14, .regular), ink: muted, centered: true)
  NSGraphicsContext.restoreGraphicsState()
  return bitmap
}

// Finder picks the 2x representation on Retina displays at the same layout size.
let representations = [render(scale: 1), render(scale: 2)]
try NSBitmapImageRep.representationOfImageReps(in: representations, using: .tiff, properties: [:])!
  .write(to: output)
