"""
Ryx ORM — Example 08: Validation & Clean

This example covers:
  - Field-level validation (max_length, min_value, email, url, choices, etc.)
  - Built-in validators (MaxLengthValidator, EmailValidator, etc.)
  - Custom validators via FunctionValidator
  - Model.clean() — cross-field validation
  - full_clean() — run all validators + model.clean()
  - save(validate=True/False) — control validation on save
  - ValidationError — string, list, dict formats
  - Custom validators with the @validator pattern

Run with:
    uv run python examples/08_validation_and_clean.py
"""

import asyncio
import os
import re
from pathlib import Path

import ryx
from ryx import (
    Model,
    CharField,
    IntField,
    FloatField,
    EmailField,
    URLField,
    ValidationError,
    FunctionValidator,
    MaxLengthValidator,
    MinValueValidator,
    MaxValueValidator,
    RegexValidator,
    ChoicesValidator,
)
from ryx.migrations import MigrationRunner


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS WITH VALIDATION
#
class Product(Model):
    """Product with various field-level validations."""

    class Meta:
        table_name = "ex8_products"

    # CharField with length constraints
    name = CharField(max_length=100, min_length=3)

    # EmailField — built-in email format validation
    contact_email = EmailField()

    # URLField — built-in URL format validation
    website = URLField(null=True, blank=True)

    # Numeric field with range
    price = IntField(min_value=0, max_value=99999)

    # Float with range
    weight = FloatField(min_value=0.0, max_value=1000.0, null=True, blank=True)

    # Custom validators on a field
    sku = CharField(
        max_length=20,
        validators=[
            RegexValidator(r"^[A-Z]{2}-\d{4}$", message="SKU must be like XX-0000"),
        ],
    )

    # Choices validator
    status = CharField(
        max_length=20,
        default="draft",
        validators=[
            ChoicesValidator(["draft", "active", "archived"]),
        ],
    )

    # Custom function validator
    description = CharField(
        max_length=500,
        null=True,
        blank=True,
        validators=[
            FunctionValidator(
                lambda v: "badword" not in (v or "").lower(),
                message="Description contains inappropriate content",
            ),
        ],
    )

    async def clean(self):
        """Cross-field validation.

        Called by full_clean() after all field validators pass.
        Raise ValidationError with a dict for field-specific errors,
        or a string/list for non-field errors.
        """
        # Premium products must have a website
        if self.price > 1000 and not self.website:
            raise ValidationError(
                {
                    "website": ["Premium products (price > 1000) must have a website"],
                }
            )

        # SKU prefix must match category (hypothetical rule)
        if self.sku and self.name and self.sku[0] != self.name[0].upper():
            raise ValidationError(
                {
                    "sku": [
                        f"SKU should start with '{self.name[0].upper()}' to match product name"
                    ],
                }
            )


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Product])
    await runner.migrate()

    # Clean
    await Product.objects.bulk_delete()


#
#  FIELD-LEVEL VALIDATION
#
async def demo_field_validation() -> None:
    print("\n" + "=" * 60)
    print("Field-Level Validation")
    print("=" * 60)

    # Valid product
    valid = Product(
        name="Laptop",
        contact_email="seller@example.com",
        price=999,
        weight=2.5,
        sku="LP-1234",
        status="draft",
    )
    await valid.full_clean()
    print(f"Valid product passed: {valid.name}")

    # Invalid: name too short
    try:
        bad_name = Product(
            name="AB", contact_email="test@example.com", price=10, sku="XX-0000"
        )
        await bad_name.full_clean()
    except ValidationError as e:
        print(f"Name too short: {e.errors}")

    # Invalid: bad email
    try:
        bad_email = Product(
            name="Widget", contact_email="not-an-email", price=10, sku="XX-0000"
        )
        await bad_email.full_clean()
    except ValidationError as e:
        print(f"Bad email: {e.errors}")

    # Invalid: price out of range
    try:
        bad_price = Product(
            name="Widget", contact_email="test@example.com", price=-5, sku="XX-0000"
        )
        await bad_price.full_clean()
    except ValidationError as e:
        print(f"Negative price: {e.errors}")

    # Invalid: bad SKU format
    try:
        bad_sku = Product(
            name="Widget", contact_email="test@example.com", price=10, sku="invalid"
        )
        await bad_sku.full_clean()
    except ValidationError as e:
        print(f"Bad SKU: {e.errors}")

    # Invalid: bad status choice
    try:
        bad_status = Product(
            name="Widget",
            contact_email="test@example.com",
            price=10,
            sku="XX-0000",
            status="deleted",
        )
        await bad_status.full_clean()
    except ValidationError as e:
        print(f"Bad status: {e.errors}")


#
#  CROSS-FIELD VALIDATION (model.clean)
#
async def demo_cross_field_validation() -> None:
    print("\n" + "=" * 60)
    print("Cross-Field Validation (model.clean)")
    print("=" * 60)

    # Premium product without website — should fail clean()
    try:
        premium = Product(
            name="Enterprise Server",
            contact_email="sales@example.com",
            price=5000,
            weight=50.0,
            sku="ES-0001",
            # website is missing
        )
        await premium.full_clean()
    except ValidationError as e:
        print(f"Premium without website: {e.errors}")

    # Premium product WITH website — should pass
    premium_ok = Product(
        name="Enterprise Server",
        contact_email="sales@example.com",
        price=5000,
        weight=50.0,
        sku="ES-0001",
        website="https://example.com",
    )
    await premium_ok.full_clean()
    print(f"Premium with website passed: {premium_ok.name}")


#
#  SAVE WITH / WITHOUT VALIDATION
#
async def demo_save_validation() -> None:
    print("\n" + "=" * 60)
    print("save(validate=True/False)")
    print("=" * 60)

    # save(validate=True) — default, runs full_clean() before SQL
    try:
        bad = Product(
            name="AB",  # too short
            contact_email="bad",
            price=10,
            sku="XX-0000",
        )
        await bad.save()  # validate=True by default
    except ValidationError as e:
        print(f"save() with bad data: {e.errors}")

    # save(validate=False) — skips validation (use with caution)
    # Useful for bulk imports or data migration
    bypassed = Product(
        name="AB",
        contact_email="bad",
        price=10,
        sku="XX-0000",
    )
    await bypassed.save(validate=False)
    print(f"save(validate=False) bypassed validation, pk={bypassed.pk}")

    # Clean up
    await bypassed.delete()


#
#  VALIDATION ERROR FORMATS
#
async def demo_validation_error_formats() -> None:
    print("\n" + "=" * 60)
    print("ValidationError Formats")
    print("=" * 60)

    # String — non-field error
    try:
        raise ValidationError("Something went wrong")
    except ValidationError as e:
        print(f"String error: {e.errors}")

    # List — multiple non-field errors
    try:
        raise ValidationError(["Error 1", "Error 2"])
    except ValidationError as e:
        print(f"List error: {e.errors}")

    # Dict — field-specific errors
    try:
        raise ValidationError(
            {
                "name": ["Too short", "Invalid characters"],
                "email": ["Not a valid email"],
                "__all__": ["Cross-field error"],
            }
        )
    except ValidationError as e:
        print(f"Dict error: {e.errors}")

    # Merge — combine two ValidationErrors
    try:
        err1 = ValidationError({"name": ["Too short"]})
        err2 = ValidationError({"email": ["Invalid"]})
        err1.merge(err2)
        raise err1
    except ValidationError as e:
        print(f"Merged error: {e.errors}")


#
#  CUSTOM VALIDATORS
#

from ryx.validators import Validator


class SlugValidator(Validator):
    """Custom validator that enforces URL-safe slug format."""

    def __call__(self, value):
        if value and not re.match(r"^[a-z0-9]+(?:-[a-z0-9]+)*$", value):
            raise ValidationError(
                f"'{value}' is not a valid slug. Use lowercase letters, numbers, and hyphens."
            )


class EvenNumberValidator(Validator):
    """Custom validator that requires even numbers."""

    def __call__(self, value):
        if value is not None and value % 2 != 0:
            raise ValidationError(f"{value} is not an even number")


async def demo_custom_validators() -> None:
    print("\n" + "=" * 60)
    print("Custom Validators")
    print("=" * 60)

    # SlugValidator
    slug_val = SlugValidator()
    try:
        slug_val("valid-slug")
        print("valid-slug: passed")
    except ValidationError as e:
        print(f"valid-slug: failed — {e}")

    try:
        slug_val("Invalid Slug!")
    except ValidationError as e:
        print(f"Invalid Slug!: failed — {e}")

    # EvenNumberValidator
    even_val = EvenNumberValidator()
    try:
        even_val(42)
        print("42: passed (even)")
    except ValidationError as e:
        print(f"42: failed — {e}")

    try:
        even_val(7)
    except ValidationError as e:
        print(f"7: failed — {e}")


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 08: Validation & Clean")
    await setup()

    await demo_field_validation()
    await demo_cross_field_validation()
    await demo_save_validation()
    await demo_validation_error_formats()
    await demo_custom_validators()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
