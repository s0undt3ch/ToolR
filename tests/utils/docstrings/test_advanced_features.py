"""Advanced docstring parsing tests."""

from __future__ import annotations

from toolr.utils._docstrings import Docstring

LONG_DOCSTRING_FULL_DESCRIPTION = """\
Generate comprehensive test data for Stripe sandbox accounts.

This function creates a complete test environment with products, customers,
subscriptions, and payment methods. Only test keys (sk_test_*) are accepted
for safety reasons.

Random Example:
This is a random example.
```python
result = process_data("input.txt")
```

Examples:

- Basic usage - creates 5 products, 20 customers, 1 subscription each:

```
result = generate_test_data("sk_test_xxx")
```

- Custom quantities:

```
from foobar import generate_test_data

result = generate_test_data(
    "sk_test_xxx",
    products=3,
    customers=10,
    subscriptions_per_customer=2
)
```

- Full test suite with all features:

```
result = generate_test_data(
    "sk_test_xxx",
    products=10,
    customers=50,
    subscriptions_per_customer=3,
    include_payment_methods=True,
    include_coupons=True
)
```

Notes:

- Only test keys (sk_test_*) are accepted for safety.
- This function creates realistic test data for development and testing.
- The generated data follows Stripe's best practices for test scenarios.
- All created objects are automatically cleaned up after 24 hours in sandbox mode.

Warnings:

- This function will create real objects in your Stripe sandbox account.
- Make sure you're using a test key to avoid charges.

See Also:

- stripe.Customer.create: For creating individual customers
- stripe.Product.create: For creating individual products
- stripe.Subscription.create: For creating individual subscriptions

References:

- Stripe API Documentation: https://stripe.com/docs/api
- Test Data Best Practices: https://stripe.com/docs/testing

Todo:

- Add support for creating webhooks
- Implement data validation before creation
- Add progress callbacks for long-running operations

Deprecated:
The 'legacy_mode' parameter is deprecated and will be removed in v2.0.

Version Added: 1.0.0

Version Changed:
- 1.2.0: Added support for payment methods and coupons
- 1.5.0: Improved error handling and progress reporting
"""

LONG_DOCSTRING = """\
Generate comprehensive test data for Stripe sandbox accounts.

This function creates a complete test environment with products, customers,
subscriptions, and payment methods. Only test keys (sk_test_*) are accepted
for safety reasons.

Random Example:
    This is a random example.
    ```python
    result = process_data("input.txt")
    ```

Args:
    stripe_secret_key: Stripe secret key (must be a test key starting with sk_test_)
    products: The number of products to create (default: 5)
    customers: The number of customers to create (default: 20)
    subscriptions_per_customer: The number of subscriptions per customer (default: 1)
    include_payment_methods: Whether to create payment methods (default: True)
    include_coupons: Whether to create discount coupons (default: False)
    test_mode: Run in test mode with reduced data (default: False)

Returns:
    dict: Comprehensive summary containing:
        - products: List of created product IDs
        - customers: List of created customer IDs
        - subscriptions: List of created subscription IDs
        - payment_methods: List of created payment method IDs
        - coupons: List of created coupon IDs (if enabled)
        - total_amount: Total value of all subscriptions
        - metadata: Additional information about the test data


Raises:
    ValueError: If the secret key is not a test key or parameters are invalid
    ConnectionError: If unable to connect to Stripe API
    TimeoutError: If the operation takes too long
    RateLimitError: If Stripe API rate limits are exceeded


Yields:
    dict: Progress updates during data creation process

Examples:
    Basic usage - creates 5 products, 20 customers, 1 subscription each:
    ```python
        result = generate_test_data("sk_test_xxx")
    ```

    Custom quantities:
    ```python
        from foobar import generate_test_data

        result = generate_test_data(
            "sk_test_xxx",
            products=3,
            customers=10,
            subscriptions_per_customer=2
        )
    ```

    Full test suite with all features:
    ```python
        result = generate_test_data(
            "sk_test_xxx",
            products=10,
            customers=50,
            subscriptions_per_customer=3,
            include_payment_methods=True,
            include_coupons=True
        )
    ```

Notes:
    Only test keys (sk_test_*) are accepted for safety.
    This function creates realistic test data for development and testing.
    The generated data follows Stripe's best practices for test scenarios.
    All created objects are automatically cleaned up after 24 hours in sandbox mode.


Warnings:
    This function will create real objects in your Stripe sandbox account.
    Make sure you're using a test key to avoid charges.

See Also:
    stripe.Customer.create: For creating individual customers
    stripe.Product.create: For creating individual products
    stripe.Subscription.create: For creating individual subscriptions

References:
    - Stripe API Documentation: https://stripe.com/docs/api
    - Test Data Best Practices: https://stripe.com/docs/testing

Todo:
    - Add support for creating webhooks
    - Implement data validation before creation
    - Add progress callbacks for long-running operations

Deprecated:
    The 'legacy_mode' parameter is deprecated and will be removed in v2.0.

Version Added:
    1.0.0

Version Changed:
    1.2.0: Added support for payment methods and coupons
    1.5.0: Improved error handling and progress reporting
"""


def test_docstring_with_multiple_notes():
    """Test parsing a docstring with multiple notes."""
    docstring = """Function with multiple notes.

    Notes:
        This is the first note.
        This is the second note with more details.
        This is the third note.
    """
    result = Docstring.parse(docstring)

    notes = result.notes
    assert len(notes) == 3
    assert "This is the first note." in notes
    assert "This is the second note with more details." in notes
    assert "This is the third note." in notes


def test_docstring_with_version_info():
    """Test parsing a docstring with version information."""
    docstring = """Function with version information.

    Version Added:
        1.0.0

    Version Changed:
        1.1.0: Added new parameter
        1.2.0: Fixed bug in processing
    """
    result = Docstring.parse(docstring)

    assert result.version_added == "1.0.0"
    assert len(result.version_changed) == 2
    assert result.version_changed[0].version == "1.1.0"
    assert result.version_changed[0].description == "Added new parameter"
    assert result.version_changed[1].version == "1.2.0"
    assert result.version_changed[1].description == "Fixed bug in processing"


def test_docstring_with_deprecated_info():
    """Test parsing a docstring with deprecated information."""
    docstring = """Function with deprecated parameter.

    Deprecated:
        The 'old_param' parameter is deprecated and will be removed in v2.0.
    """
    result = Docstring.parse(docstring)

    deprecated = result.deprecated
    assert deprecated is not None
    assert "old_param" in deprecated


def test_large_docstring_complete():  # noqa: PLR0915
    """Test parsing a large, comprehensive docstring with all sections."""
    result = Docstring.parse(LONG_DOCSTRING)

    # Check short description
    assert result.short_description == "Generate comprehensive test data for Stripe sandbox accounts."

    # Check long description
    long_desc = result.long_description
    assert "This function creates a complete test environment" in long_desc
    assert "Only test keys (sk_test_*) are accepted" in long_desc
    # Make sure what could be confused as a section, but is not, is included in the long description
    assert "Random Example:" in long_desc
    assert "This is a random example." in long_desc

    # We don't parse returns, raises, yields, attributes
    # Make sure they are not in the long description
    assert "Returns:" not in long_desc
    assert "Raises:" not in long_desc
    assert "Yields:" not in long_desc
    assert "Attributes:" not in long_desc

    # Check parameters
    params = result.params
    assert len(params) == 7
    expected_params = [
        "stripe_secret_key",
        "products",
        "customers",
        "subscriptions_per_customer",
        "include_payment_methods",
        "include_coupons",
        "test_mode",
    ]
    for param in expected_params:
        assert param in params

    # Check examples - Rust parser doesn't parse examples the same way
    examples = result.examples
    assert len(examples) == 3
    assert examples[0].description == "Basic usage - creates 5 products, 20 customers, 1 subscription each:"
    assert examples[0].snippet is not None
    assert examples[1].description == "Custom quantities:"
    assert examples[1].snippet is not None
    assert examples[2].description == "Full test suite with all features:"
    assert examples[2].snippet is not None

    # Check notes
    notes = result.notes
    assert len(notes) == 4
    assert "Only test keys (sk_test_*) are accepted for safety." in notes
    assert "This function creates realistic test data for development and testing." in notes

    # Check warnings
    warnings = result.warnings
    assert len(warnings) == 2
    assert "This function will create real objects" in warnings[0]
    assert "Make sure you're using a test key" in warnings[1]

    # Check see also
    see_also = result.see_also
    assert len(see_also) == 3
    assert "stripe.Customer.create" in see_also[0]
    assert "stripe.Product.create" in see_also[1]
    assert "stripe.Subscription.create" in see_also[2]

    # Check references
    references = result.references
    assert len(references) == 2
    assert "Stripe API Documentation" in references[0]
    assert "Test Data Best Practices" in references[1]

    # Check todo
    todo = result.todo
    assert len(todo) == 3
    assert "Add support for creating webhooks" in todo[0]
    assert "Implement data validation" in todo[1]
    assert "Add progress callbacks" in todo[2]

    # Check deprecated
    deprecated = result.deprecated
    assert deprecated is not None
    assert "legacy_mode" in deprecated

    # Check version info
    assert result.version_added == "1.0.0"
    assert len(result.version_changed) == 2
    assert result.version_changed[0].version == "1.2.0"
    assert result.version_changed[0].description == "Added support for payment methods and coupons"
    assert result.version_changed[1].version == "1.5.0"
    assert result.version_changed[1].description == "Improved error handling and progress reporting"


def test_full_description():
    """Test the full description of a docstring."""
    result = Docstring.parse(LONG_DOCSTRING)
    assert result.full_description == LONG_DOCSTRING_FULL_DESCRIPTION
