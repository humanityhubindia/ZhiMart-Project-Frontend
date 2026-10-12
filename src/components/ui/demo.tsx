import { PlaceCard } from '@/components/ui/card-22';

// Sample data to be passed as props
const demoPlaceData = {
  images: [
    'https://images.unsplash.com/photo-1596622247990-84877175438a?q=80&w=1200&auto=format&fit=crop',
    'https://images.unsplash.com/photo-1544644181-1484b3fdfc62?q=80&w=1200&auto=format&fit=crop',
    'https://images.unsplash.com/photo-1518709268805-4e9042af9f23?q=80&w=1200&auto=format&fit=crop',
  ],
  tags: ['Adventure', 'Ancient Monuments'],
  rating: 4.8,
  title: 'Petra, Jordan',
  dateRange: 'May 1 - 6',
  hostType: 'Business host',
  isTopRated: true,
  description: 'A lost city carved in rose-colored stone, hidden in majestic desert canyons.',
  pricePerNight: 139,
};

const PlaceCardDemo = () => {
  return (
    <div className="flex min-h-[500px] w-full items-center justify-center bg-background p-4">
      <PlaceCard
        images={demoPlaceData.images}
        tags={demoPlaceData.tags}
        rating={demoPlaceData.rating}
        title={demoPlaceData.title}
        dateRange={demoPlaceData.dateRange}
        hostType={demoPlaceData.hostType}
        isTopRated={demoPlaceData.isTopRated}
        description={demoPlaceData.description}
        pricePerNight={demoPlaceData.pricePerNight}
      />
    </div>
  );
};

export default PlaceCardDemo;
