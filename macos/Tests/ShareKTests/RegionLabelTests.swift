import Foundation
import Testing
@testable import ShareK

private let english = Locale(identifier: "en_US")

@Test func theLabelJoinsPixelsRatioAndScreenShare() {
    #expect(RegionLabel.text(width: 1920, height: 1080, screenWidth: 3840, screenHeight: 2160, locale: english) == " 1920 × 1080 px · 16:9 · 25 % ")
}

@Test func anEmptySelectionShowsOnlyItsPixels() {
    #expect(RegionLabel.text(width: 0, height: 540, screenWidth: 3840, screenHeight: 2160) == " 0 × 540 px ")
}

@Test func commonRatiosKeepTheirUsualNames() {
    #expect(RegionLabel.ratio(width: 1440, height: 900) == "16:10")
    #expect(RegionLabel.ratio(width: 1080, height: 1920) == "9:16")
}

@Test func otherRatiosWithSmallTermsAreExact() {
    #expect(RegionLabel.ratio(width: 700, height: 500) == "7:5")
}

@Test func ratiosWithinOnePercentOfACommonOneAreApproximated() {
    #expect(RegionLabel.ratio(width: 1000, height: 557) == "≈ 16:9")
    #expect(RegionLabel.ratio(width: 1000, height: 556, locale: english) == "1.80:1")
}

@Test func portraitRatiosWithLargeTermsAreDecimalOverOne() {
    #expect(RegionLabel.ratio(width: 556, height: 1000, locale: english) == "1:1.80")
}

@Test func partialSelectionsStayBetweenOneAndNinetyNinePercent() {
    #expect(RegionLabel.share(1, of: 3840 * 2160) == "1 %")
    #expect(RegionLabel.share(3839 * 2160, of: 3840 * 2160) == "99 %")
    #expect(RegionLabel.share(3840 * 2160, of: 3840 * 2160) == "100 %")
}
