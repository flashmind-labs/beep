package com.flashmind.beep

import org.junit.Assert.*
import org.junit.Test

class ProductTest {
    @Test fun readsEveryField() {
        val p = parseProduct("""{"title":"Stick vacuum","url":"https://shop.example/item","price":"€249","was":"€329","store":"Shop","image":"https://shop.example/img.jpg","rating":4.6,"reviews":1204,"note":"Quietest"}""")!!
        assertEquals("Stick vacuum", p.title)
        assertEquals("€249", p.price)
        assertEquals("€329", p.was)
        assertEquals("Shop", p.store)
        assertEquals("https://shop.example/img.jpg", p.image)
        assertEquals(4.6, p.rating!!, 0.0)
        assertEquals(1204, p.reviews)
        assertEquals("Quietest", p.note)
    }

    @Test fun storeDefaultsToHost() {
        val p = parseProduct("""{"title":"A","url":"https://www.shop.example/a"}""")!!
        assertEquals("shop.example", p.store)
        assertNull(p.price)
        assertNull(p.details)
    }

    @Test fun titleAndUrlAreRequired() {
        assertNull(parseProduct("""{"url":"https://shop.example/a"}"""))
        assertNull(parseProduct("""{"title":"A"}"""))
        assertNull(parseProduct("""{"title":"A","url":"javascript:alert(1)"}"""))
        assertNull(parseProduct("""{"title":"A","url":"https://shop.example/a""""))
        assertNull(parseProduct("not json"))
    }

    @Test fun detailsLine() {
        val p = parseProduct("""{"title":"A","url":"https://shop.example/a","rating":4,"note":"Light"}""")!!
        assertEquals("4 ★ · Light", p.details)
    }
}
