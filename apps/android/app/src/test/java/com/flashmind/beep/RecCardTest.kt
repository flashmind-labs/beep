package com.flashmind.beep

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class RecCardTest {
    @Test fun readsEachKindAndSkipsBadCards() {
        val cards = shownRecCards(JSONObject("""{"action":"show_cards","cards":[
            {"kind":"place","title":"La Pubilla","url":"https://lapubilla.example/","site":"lapubilla.example","facts":["€€"],"rating":4.6,"reviews":2310,"note":"Closest",
             "place":{"address":"Plaça de la Llibertat 23","latitude":41.4,"longitude":2.15,"distance_m":648,"hours":"Open until 23:00","open_now":true,"booking_url":"https://book.example/p"}},
            {"kind":"media","title":"Aftersun","url":"https://films.example/a","media":{"year":2022,"creator":"Charlotte Wells","watch":"Filmin","watch_url":"https://filmin.example/a"}},
            {"kind":"event","title":"Trio","url":"https://jazz.example/","event":{"start":"2026-10-17T21:00:00+02:00","venue":"Jamboree"}},
            {"kind":"place","title":"Bad","url":"javascript:alert(1)"},
            {"kind":"place","url":"https://x.example/"}
        ],"dropped":[]}"""))
        assertEquals(listOf("La Pubilla", "Aftersun", "Trio"), cards.map { it.title })
        val place = cards[0].place!!
        assertEquals(648, place.distanceMeters)
        assertEquals(true, place.openNow)
        assertEquals("https://book.example/p", place.bookingUrl)
        assertEquals("4.6 ★ (2,310) · Closest", cards[0].details?.replace(' ', ' '))
        assertEquals("Filmin", cards[1].media!!.watch)
        assertEquals(17, cards[2].event!!.start.dayOfMonth)
        assertNull(cards[2].place)
    }

    @Test fun distancesRoundLikeAPersonWould() {
        assertEquals("650 m", distanceText(648))
        assertEquals("13 km", distanceText(12_600))
    }
}
