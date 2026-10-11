package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import com.flashmind.beep.proto.Place as ProtoPlace

class PlacesTest {
    private val gym = "0b6f3c1e-2a4d-4e8f-9c3b-1d2e3f4a5b6c"

    @Test
    fun readsPlaceSchedules() {
        assertEquals(PlaceSchedule(PlaceEvent.Arrive, gym, PlaceRepeat.Always), placeSchedule("arrive:$gym"))
        assertEquals(PlaceSchedule(PlaceEvent.Leave, gym, PlaceRepeat.Daily), placeSchedule("leave:$gym:daily"))
        assertEquals(PlaceSchedule(PlaceEvent.Leave, gym, PlaceRepeat.Once), placeSchedule("leave:$gym:once"))
        assertNull(placeSchedule("0 9 * * *"))
        assertNull(placeSchedule("2026-10-12T09:00:00+02:00"))
        assertNull(placeSchedule("arrive:"))
        assertNull(placeSchedule("arrive:$gym:weekly"))
        assertNull(placeSchedule("arrive:$gym:once:daily"))
    }

    @Test
    fun writesPlaceSchedules() {
        assertEquals("arrive:$gym:daily", PlaceSchedule(PlaceEvent.Arrive, gym, PlaceRepeat.Daily).toString())
        assertEquals("leave:$gym", PlaceSchedule(PlaceEvent.Leave, gym, PlaceRepeat.Always).toString())
        assertEquals("leave:$gym:once", PlaceSchedule(PlaceEvent.Leave, gym, PlaceRepeat.Once).toString())
    }

    @Test
    fun describesPlaceSchedules() {
        val names = mapOf(gym to "Gym")
        assertEquals("When you arrive at Gym · once a day at most", describePlaceSchedule(placeSchedule("arrive:$gym:daily")!!, names))
        assertEquals("When you leave Gym", describePlaceSchedule(placeSchedule("leave:$gym")!!, names))
        assertEquals("When you leave Gym · next time only", describePlaceSchedule(placeSchedule("leave:$gym:once")!!, names))
        assertEquals("When you leave a saved place", describePlaceSchedule(placeSchedule("leave:other")!!, names))
    }

    @Test
    fun mapsServerPlaces() {
        val server = ProtoPlace(id = gym, name = "Gym", latitude = 41.39, longitude = 2.17, radius = 100u, address = null,
            createdAt = "2026-10-01T08:00:00Z", here = false, lastHereAt = "2026-10-09T18:30:00Z", routines = 2u)
        assertEquals(Place(gym, "Gym", "", 100, 2, 1_791_570_600_000L, false), appPlace(server))
        assertNull(appPlace(server.copy(lastHereAt = null)).lastHere)
        assertEquals("Main St 1", appPlace(server.copy(address = "Main St 1")).address)
    }

    @Test
    fun savedPlaceCard() {
        val result = """{"saved":true,"moved":false,"place":{"id":"$gym","name":"Gym","address":"Main St 1","radius_m":100,
            "latitude":41.39,"longitude":2.17,"here_now":true,"last_here_at":null,"routines":0,"distance_m":3}}"""
        val card = cardFor(ToolCall("c1", "place_save", isDone = true, result = result))
        assertEquals(ToolCard.SavedPlace(gym, "Gym", "Main St 1", 100, 41.39, 2.17), card)
        val held = """{"saved":false,"nearby":"Home","distance_m":40,"next":"Ask the user"}"""
        assertNull(cardFor(ToolCall("c2", "place_save", isDone = true, result = held)))
    }
}
