package shop;

import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.SortedSet;

public class Shop {
    private Customer owner;
    protected List<Order> orders;
    public Set<Product> catalog;
    private SortedSet<Product> ranked;
    private Map<String, Customer> byName;
    private Optional<Order> current;
    private Product[] shelf;
    private static final Customer NOBODY = null;
    // No associations: values, primitives, template parameters, collections of values.
    private String name;
    private int count;
    private List<String> tags;
    private Map<Customer, String> notes;
}

class Customer {
}

class Order {
}

class Product {
}

class Box<T> {
    private T content;
    private List<T> more;
}
