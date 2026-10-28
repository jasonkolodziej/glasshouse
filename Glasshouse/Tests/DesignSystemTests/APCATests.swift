import Testing
@testable import DesignSystem

@Suite("APCA contrast")
struct APCATests {
    private func gray(_ v: Int) -> APCA.RGB { APCA.RGB(Double(v) / 255, Double(v) / 255, Double(v) / 255) }

    // Reference values from the APCA documentation / apca-w3 (0.0.98G-4g).
    @Test func referencePoints() {
        #expect(abs(APCA.contrast(text: gray(0), background: gray(255)) - 106.04) < 0.05)
        #expect(abs(APCA.contrast(text: gray(255), background: gray(0)) - (-107.88)) < 0.05)
        #expect(abs(APCA.contrast(text: gray(0x88), background: gray(255)) - 63.06) < 0.05)
        #expect(abs(APCA.contrast(text: gray(255), background: gray(0x88)) - (-68.54)) < 0.05)
    }

    @Test func identicalColorsHaveNoContrast() {
        #expect(APCA.contrast(text: gray(120), background: gray(120)) == 0)
    }

    @Test func enforceRaisesDimRedOnDarkBackground() {
        let bg = APCA.RGB(0x16 / 255, 0x18 / 255, 0x1D / 255)
        let dimRed = APCA.RGB(0x80 / 255, 0.1, 0.1)
        let fixed = APCA.enforce(minimum: 60, text: dimRed, background: bg)
        #expect(abs(APCA.contrast(text: fixed, background: bg)) >= 60)
        #expect(fixed.r > fixed.g) // still reads as red
    }

    @Test func enforceLeavesPassingColorsAlone() {
        let c = gray(240)
        #expect(APCA.enforce(minimum: 75, text: c, background: gray(10)) == c)
    }
}
