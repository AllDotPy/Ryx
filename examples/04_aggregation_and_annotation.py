"""
Ryx ORM — Example 04: Aggregation & Annotation

This example covers:
  - Count, Sum, Avg, Min, Max aggregates
  - .aggregate() — return a single dict of aggregate values
  - .annotate() — attach aggregate expressions to each row
  - .values() — restrict SELECT columns + enable GROUP BY
  - Combined annotate + values for GROUP BY queries
  - distinct aggregates
  - RawAgg for custom SQL expressions

Run with:
    uv run python examples/04_aggregation_and_annotation.py
"""

import asyncio
import os
from pathlib import Path

import ryx
from ryx import (
    Model,
    CharField,
    IntField,
    ForeignKey,
    Count,
    Sum,
    Avg,
    Min,
    Max,
    RawAgg,
)
from ryx.migrations import MigrationRunner


DB_PATH = Path(__file__).parent.parent / "ryx_examples.sqlite3"
DATABASE_URL = f"sqlite://{DB_PATH}?mode=rwc"
os.environ["RYX_DATABASE_URL"] = DATABASE_URL


#
#  MODELS
#
class Author(Model):
    class Meta:
        table_name = "ex4_authors"

    name = CharField(max_length=100)
    department = CharField(max_length=50)


class Book(Model):
    class Meta:
        table_name = "ex4_books"

    title = CharField(max_length=200)
    pages = IntField(default=0)
    price = IntField(default=0)
    rating = IntField(default=0, min_value=0, max_value=5)
    author = ForeignKey(Author, on_delete="CASCADE")


async def setup() -> None:
    await ryx.setup(DATABASE_URL)
    runner = MigrationRunner([Author, Book])
    await runner.migrate()

    # Clean and seed
    await Book.objects.bulk_delete()
    await Author.objects.bulk_delete()

    alice = await Author.objects.create(name="Alice", department="Science")
    bob = await Author.objects.create(name="Bob", department="Science")
    carol = await Author.objects.create(name="Carol", department="Arts")

    books = [
        Book(title="Quantum Physics", pages=400, price=50, rating=5, author=alice),
        Book(title="Biology 101", pages=300, price=35, rating=4, author=alice),
        Book(title="Organic Chem", pages=350, price=45, rating=3, author=bob),
        Book(title="Data Structures", pages=500, price=60, rating=5, author=bob),
        Book(title="Art History", pages=200, price=30, rating=4, author=carol),
        Book(title="Modern Poetry", pages=150, price=20, rating=3, author=carol),
    ]
    await Book.objects.bulk_create(books)


#
#  AGGREGATE — Single dict of aggregate values
#
async def demo_aggregate() -> None:
    print("\n" + "=" * 60)
    print("aggregate() — Single dict of aggregates")
    print("=" * 60)

    # Basic aggregates on the entire table
    stats = await Book.objects.aggregate(
        total_books=Count("id"),
        total_pages=Sum("pages"),
        avg_price=Avg("price"),
        min_price=Min("price"),
        max_price=Max("price"),
    )
    print("All books:")
    print(f"  Count:    {stats['total_books']}")
    print(f"  Pages:    {stats['total_pages']}")
    print(f"  Avg price: ${stats['avg_price']:.1f}")
    print(f"  Min price: ${stats['min_price']}")
    print(f"  Max price: ${stats['max_price']}")

    # Aggregates on a filtered queryset
    science_stats = await Book.objects.filter(author__department="Science").aggregate(
        science_books=Count("id"),
        science_pages=Sum("pages"),
    )
    print("\nScience books:")
    print(f"  Count: {science_stats['science_books']}")
    print(f"  Pages: {science_stats['science_pages']}")

    # COUNT(*) shorthand
    total = await Book.objects.aggregate(total=Count())
    print(f"\nCOUNT(*): {total['total']}")


#
#  ANNOTATE — Attach aggregates to each row
#
async def demo_annotate() -> None:
    print("\n" + "=" * 60)
    print("annotate() — Attach aggregates to each row")
    print("=" * 60)

    # Annotate each book with how many pages above average it has
    # (This is a simplified example — real subqueries would need more complex SQL)
    books = await Book.objects.annotate(
        page_tier=Count("id")  # Just demonstrating the API
    )
    print(f"Annotated books: {len(books)}")
    for b in books[:3]:
        print(f"  {b.title}: {b}")


#
#  VALUES + GROUP BY
#
async def demo_values_group_by() -> None:
    print("\n" + "=" * 60)
    print("values() + annotate() — GROUP BY queries")
    print("=" * 60)

    # Note: .values() + .annotate() returns dicts grouped by the specified fields.
    # The Rust side handles this by returning raw dicts when .values() is used.
    # For now, we demonstrate the concept with aggregate on filtered queries.

    # Count books per author using filtered aggregates
    authors = await Author.objects.all()
    print("Books per author:")
    for author in authors:
        count = await Book.objects.filter(author_id=author.pk).count()
        print(f"  {author.name}: {count} books")

    # Average price by category
    categories = ["Science", "Arts"]
    print("\nAverage price by department:")
    for dept in categories:
        result = await Book.objects.filter(author__department=dept).aggregate(
            avg_price=Avg("price")
        )
        avg = result.get("avg_price")
        if avg is not None:
            print(f"  {dept}: ${avg:.1f}")
        else:
            print(f"  {dept}: No books")


#
#  DISTINCT AGGREGATES
#
async def demo_distinct_aggregates() -> None:
    print("\n" + "=" * 60)
    print("Distinct aggregates")
    print("=" * 60)

    # Count distinct ratings
    distinct_ratings = await Book.objects.aggregate(
        unique_ratings=Count("rating", distinct=True)
    )
    print(f"Distinct rating values: {distinct_ratings['unique_ratings']}")

    # Sum of distinct prices
    distinct_prices = await Book.objects.aggregate(
        sum_distinct_prices=Sum("price", distinct=True)
    )
    print(f"Sum of distinct prices: ${distinct_prices['sum_distinct_prices']}")


#
#  RAW AGGREGATE
#
async def demo_raw_agg() -> None:
    print("\n" + "=" * 60)
    print("RawAgg — Custom SQL aggregate expressions")
    print("=" * 60)

    # Custom aggregate: average pages rounded to nearest 10
    result = await Book.objects.aggregate(
        avg_pages_rounded=RawAgg("ROUND(AVG(pages), -1)", alias="avg_pages_rounded")
    )
    print(f"Average pages (rounded to 10s): {result.get('avg_pages_rounded', 'N/A')}")


#
#  MAIN
#
async def main() -> None:
    print("Ryx ORM — Example 04: Aggregation & Annotation")
    await setup()

    await demo_aggregate()
    await demo_annotate()
    await demo_values_group_by()
    await demo_distinct_aggregates()
    await demo_raw_agg()

    print("\nDone!")


if __name__ == "__main__":
    asyncio.run(main())
